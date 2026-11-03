//! Seekable MKV audio compatibility copies. Originals and ASS attachments stay untouched.
use crate::{auth, bad_request, AppError, AppState, MediaRequest};
use axum::{
    extract::{Path, Query, Request, State},
    http::StatusCode,
    response::Response,
    Extension, Json,
};
use futures_util::StreamExt;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    path::PathBuf,
    process::Stdio,
    sync::{Arc, Mutex, Weak},
    time::{Duration, SystemTime},
};
use tokio::io::AsyncReadExt;

struct Root(PathBuf);
impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
struct Copy {
    root: Arc<Root>,
    id: String,
    key: String,
    index: u32,
    bytes: u64,
}
impl Copy {
    fn path(&self) -> PathBuf {
        self.root.0.join(format!("{}.mp4", self.id))
    }
}
impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.path());
    }
}
#[derive(Default)]
struct Cache {
    root: Option<Arc<Root>>,
    entries: Vec<Arc<Copy>>,
    alive: Vec<Weak<Copy>>,
}
pub struct Runtime {
    cache: Mutex<Cache>,
    jobs: tokio::sync::Semaphore,
}
impl Runtime {
    pub fn new() -> Self {
        Self {
            cache: Mutex::new(Cache::default()),
            jobs: tokio::sync::Semaphore::new(1),
        }
    }
}
fn busy() -> AppError {
    AppError(
        StatusCode::TOO_MANY_REQUESTS,
        "Video audio processing or temporary storage is busy. Try again shortly.".into(),
    )
}
fn failed() -> AppError {
    bad_request("Audio could not be prepared. The video must support MP4 stream copying and fit the temporary storage and processing limits.")
}
fn limit() -> u64 {
    std::env::var("GRIMOIRE_VIDEO_CACHE_MIB")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|v| (64..=65536).contains(v))
        .unwrap_or(8192)
        * 1024
        * 1024
}
fn key(file: &std::path::Path, source: &str, index: u32) -> Result<String, AppError> {
    let meta = std::fs::metadata(file).map_err(|_| failed())?;
    let modified = meta
        .modified()
        .map_err(|_| failed())?
        .duration_since(SystemTime::UNIX_EPOCH)
        .map_err(|_| failed())?
        .as_nanos();
    Ok(hex::encode(Sha256::digest(format!(
        "{source}\0{}\0{}\0{modified}\0{index}",
        file.display(),
        meta.len()
    ))))
}
async fn file(
    state: &AppState,
    user: &auth::User,
    source: &str,
    path: &str,
) -> Result<PathBuf, AppError> {
    if path.len() > 4096 {
        return Err(failed());
    }
    let root = crate::media_root(state, user, source).await?;
    crate::resolve_media_path(&root, path, "video")
}
#[derive(Serialize)]
pub struct Track {
    index: u32,
    label: String,
    codec: String,
    default: bool,
}
fn tracks(probe: &serde_json::Value) -> Vec<Track> {
    probe["streams"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|s| s["codec_type"] == "audio")
        .filter_map(|s| {
            Some(Track {
                index: u32::try_from(s["index"].as_u64()?).ok()?,
                label: [s["tags"]["language"].as_str(), s["tags"]["title"].as_str()]
                    .into_iter()
                    .flatten()
                    .map(|s| s.chars().take(128).collect::<String>())
                    .collect::<Vec<_>>()
                    .join(" · "),
                codec: s["codec_name"].as_str().unwrap_or("unknown").to_owned(),
                default: s["disposition"]["default"] == 1,
            })
        })
        .collect()
}
pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<MediaRequest>,
) -> Result<Json<Vec<Track>>, AppError> {
    let path = file(&state, &identity.user, &source, &query.path).await?;
    if !path
        .extension()
        .is_some_and(|v| v.eq_ignore_ascii_case("mkv"))
    {
        return Ok(Json(vec![]));
    }
    auth::rate_limit(
        &state.db,
        format!("video-probe:{}", identity.user.id),
        60,
        300,
    )
    .await?;
    Ok(Json(tracks(&crate::subtitles_mkv::inspect(&path).await?)))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Selection {
    stream_index: u32,
}
#[derive(Serialize)]
pub struct Prepared {
    id: String,
}
pub async fn prepare(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<MediaRequest>,
    Json(selection): Json<Selection>,
) -> Result<Json<Prepared>, AppError> {
    let path = file(&state, &identity.user, &source, &query.path).await?;
    if !path
        .extension()
        .is_some_and(|v| v.eq_ignore_ascii_case("mkv"))
    {
        return Err(failed());
    }
    auth::rate_limit(
        &state.db,
        format!("video-prepare:{}", identity.user.id),
        20,
        300,
    )
    .await?;
    // Own the permit inside the worker: disconnecting a client must not leave a live encoder outside the limit.
    let runtime = state.video_audio.clone();
    let work_state = state.clone();
    let user = identity.user.clone();
    let task = tokio::spawn(async move {
        let _permit = runtime.jobs.try_acquire().map_err(|_| busy())?;
        let fingerprint = key(&path, &source, selection.stream_index)?;
        {
            let mut cache = runtime.cache.lock().unwrap();
            if let Some(pos) = cache
                .entries
                .iter()
                .position(|entry| entry.key == fingerprint)
            {
                let entry = cache.entries.remove(pos);
                let id = entry.id.clone();
                cache.entries.push(entry);
                return Ok(Json(Prepared { id }));
            }
        }
        let probe = crate::subtitles_mkv::inspect(&path).await?;
        if !tracks(&probe)
            .iter()
            .any(|t| t.index == selection.stream_index)
        {
            return Err(bad_request("Audio track not found."));
        }
        let duration = probe["format"]["duration"]
            .as_str()
            .and_then(|v| v.parse::<f64>().ok())
            .filter(|v| v.is_finite() && *v > 0.)
            .ok_or_else(failed)?;
        let (copy, budget) = {
            let mut cache = runtime.cache.lock().unwrap();
            // Evict completed cache entries; in-flight response bodies retain their allocation.
            let needed = std::fs::metadata(&path)
                .map_err(|_| failed())?
                .len()
                .saturating_add(64 * 1024 * 1024)
                .min(limit());
            loop {
                cache.alive.retain(|entry| entry.strong_count() > 0);
                let used: u64 = cache
                    .alive
                    .iter()
                    .filter_map(Weak::upgrade)
                    .map(|c| c.bytes)
                    .sum();
                if (limit().saturating_sub(used) >= needed && cache.entries.len() < 4)
                    || cache.entries.is_empty()
                {
                    break;
                }
                cache.entries.remove(0);
            }
            let used: u64 = cache
                .alive
                .iter()
                .filter_map(Weak::upgrade)
                .map(|c| c.bytes)
                .sum();
            let budget = limit()
                .checked_sub(used)
                .filter(|v| *v >= 64 * 1024 * 1024)
                .ok_or_else(busy)?;
            let root = if let Some(root) = &cache.root {
                root.clone()
            } else {
                let directory =
                    std::env::temp_dir().join(format!("grimoire-video-{}", uuid::Uuid::new_v4()));
                let mut builder = std::fs::DirBuilder::new();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::DirBuilderExt;
                    builder.mode(0o700);
                }
                builder.create(&directory).map_err(|_| failed())?;
                let root = Arc::new(Root(directory));
                cache.root = Some(root.clone());
                root
            };
            (
                Copy {
                    root,
                    id: uuid::Uuid::new_v4().to_string(),
                    key: fingerprint.clone(),
                    index: selection.stream_index,
                    bytes: 0,
                },
                budget,
            )
        };
        let output = copy.path();
        // Precreate private file; FFmpeg truncates it but retains permissions.
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&output).map_err(|_| failed())?;
        let mut command = tokio::process::Command::new("ffmpeg");
        command
            .args([
                "-v",
                "error",
                "-nostdin",
                "-max_alloc",
                "67108864",
                "-protocol_whitelist",
                "file,pipe",
                "-f",
                "matroska",
                "-i",
            ])
            .arg(&path)
            .args([
                "-map",
                "0:v:0",
                "-map",
                &format!("0:{}", selection.stream_index),
                "-c:v",
                "copy",
                "-c:a",
                "aac",
                "-b:a",
                "192k",
                "-ac",
                "2",
                "-threads",
                "1",
                "-sn",
                "-dn",
                "-map_metadata",
                "-1",
                "-movflags",
                "+faststart",
                "-fs",
                &budget.to_string(),
                "-y",
            ])
            .arg(&output)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        let mut child = command.spawn().map_err(|_| bad_request("The API process could not start FFmpeg. Check its PATH, executable permissions and runtime libraries."))?;
        let stderr = child.stderr.take().ok_or_else(failed)?;
        let diagnostics = tokio::spawn(async move {
            let mut stream = stderr;
            let mut bytes = Vec::new();
            let _ = (&mut stream).take(65536).read_to_end(&mut bytes).await;
            let _ = tokio::io::copy(&mut stream, &mut tokio::io::sink()).await;
            String::from_utf8_lossy(&bytes).to_string()
        });
        match tokio::time::timeout(Duration::from_secs(300), child.wait()).await {
            Ok(Ok(status)) if status.success() => (),
            Ok(Ok(_)) => {
                let message = diagnostics.await.unwrap_or_default();
                return Err(encoder_error(&message));
            }
            _ => {
                let _ = child.kill().await;
                diagnostics.abort();
                return Err(bad_request("Audio preparation exceeded the five-minute processing limit or the encoder could not finish."));
            }
        }
        let _ = diagnostics.await;
        let bytes = std::fs::metadata(&output).map_err(|_| failed())?.len();
        if bytes == 0 || bytes >= budget.saturating_sub(1024 * 1024) {
            return Err(bad_request("Audio copy exceeds the temporary video cache budget. Increase GRIMOIRE_VIDEO_CACHE_MIB on the API host or use a smaller file."));
        }
        // -fs can stop successfully mid-file. Reject incomplete copies rather than serving truncated media.
        let out_duration = output_duration(&output).await?;
        if (out_duration - duration).abs() > 1.0 {
            return Err(bad_request("The audio copy is incomplete or its duration does not match the original. It may exceed the temporary storage limit."));
        }
        let checked = file(&work_state, &user, &source, &query.path).await?;
        if key(&checked, &source, selection.stream_index)? != fingerprint {
            return Err(failed());
        }
        let id = copy.id.clone();
        let mut copy = copy;
        copy.bytes = bytes;
        let copy = Arc::new(copy);
        let mut cache = runtime.cache.lock().unwrap();
        cache.alive.push(Arc::downgrade(&copy));
        cache.entries.push(copy);
        Ok(Json(Prepared { id }))
    });
    task.await.map_err(|_| failed())?
}
async fn output_duration(path: &std::path::Path) -> Result<f64, AppError> {
    let mut child = tokio::process::Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            "mov",
            "-enable_drefs",
            "0",
            "-use_absolute_path",
            "0",
            "-i",
        ])
        .arg(path)
        .args([
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true)
        .spawn()
        .map_err(|_| failed())?;
    let result = tokio::time::timeout(Duration::from_secs(30), async {
        let mut bytes = vec![];
        child
            .stdout
            .take()
            .ok_or_else(failed)?
            .take(1024)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| failed())?;
        if !child.wait().await.map_err(|_| failed())?.success() {
            return Err(failed());
        }
        String::from_utf8(bytes)
            .ok()
            .and_then(|v| v.trim().parse::<f64>().ok())
            .filter(|v| v.is_finite())
            .ok_or_else(failed)
    })
    .await;
    match result {
        Ok(value) => value,
        Err(_) => {
            let _ = child.kill().await;
            Err(failed())
        }
    }
}
pub async fn read(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, String)>,
    Query(query): Query<MediaRequest>,
    request: Request,
) -> Result<Response, AppError> {
    let path = file(&state, &identity.user, &source, &query.path).await?;
    let copy = state
        .video_audio
        .cache
        .lock()
        .unwrap()
        .entries
        .iter()
        .find(|c| c.id == id)
        .cloned()
        .ok_or_else(|| {
            AppError(
                StatusCode::NOT_FOUND,
                "Prepared audio expired. Select the track again.".into(),
            )
        })?;
    let matches = key(&path, &source, copy.index)? == copy.key;
    if !matches {
        return Err(failed());
    }
    let response = crate::serve_media_file(copy.path(), request, "video").await?;
    let (parts, body) = response.into_parts();
    let stream = body.into_data_stream().map(move |chunk| {
        let _allocation = &copy;
        chunk
    });
    Ok(Response::from_parts(
        parts,
        axum::body::Body::from_stream(stream),
    ))
}

fn encoder_error(stderr: &str) -> AppError {
    let message = if stderr.contains("No space left") {
        "The API host ran out of temporary disk space."
    } else if stderr.contains("Permission denied") {
        "FFmpeg could not read the source or write the temporary copy. Check API service permissions."
    } else if stderr.contains("not currently supported in container")
        || stderr.contains("Could not find tag for codec")
    {
        "This video codec cannot be copied into MP4. Video transcoding is required for this file; FFmpeg is installed."
    } else if stderr.contains("Unrecognized option") || stderr.contains("Option not found") {
        "The installed FFmpeg rejected a processing option. Check the API host FFmpeg version."
    } else {
        "FFmpeg started but could not prepare this file. The codec/container may not support MP4 copying, or processing limits were exceeded."
    };
    bad_request(message)
}
#[cfg(test)]
mod tests {
    #[test]
    fn encoder_errors_distinguish_installed_tools_and_do_not_expose_paths() {
        assert!(
            super::encoder_error("Could not find tag for codec /private/file")
                .1
                .contains("Video transcoding")
        );
        assert!(!super::encoder_error("failed /private/file")
            .1
            .contains("/private"));
        assert!(super::encoder_error("No space left on device")
            .1
            .contains("disk space"));
    }
}
