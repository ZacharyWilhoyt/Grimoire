//! On-demand six-second HLS segments. No whole-file copies or client-supplied encoder flags.
use crate::{auth, bad_request, AppError, AppState};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use std::{
    collections::VecDeque,
    path::{Path as FsPath, PathBuf},
    process::Stdio,
    sync::Mutex,
    time::{Duration, Instant},
};
use tokio::io::AsyncReadExt;
const SECONDS: f64 = 6.;
#[derive(Clone, Copy, Serialize, Deserialize, PartialEq, Debug)]
#[serde(rename_all = "snake_case")]
pub enum Encoder {
    Auto,
    Cpu,
    Nvidia,
    Amd,
    Amf,
}
#[derive(Clone, Serialize)]
pub struct Capability {
    encoder: Encoder,
    available: bool,
}
#[derive(Clone)]
struct Session {
    id: String,
    user: uuid::Uuid,
    source: String,
    path: String,
    fingerprint: String,
    encoder: Encoder,
    policy: Encoder,
    height: u32,
    audio: Option<u32>,
    duration: f64,
    touched: Instant,
}
pub struct Runtime {
    sessions: Mutex<Vec<Session>>,
    cache: Mutex<VecDeque<(String, u32, Bytes)>>,
    jobs: tokio::sync::Semaphore,
    capabilities: tokio::sync::OnceCell<Vec<Capability>>,
}
impl Runtime {
    pub fn new() -> Self {
        Self {
            sessions: Mutex::new(vec![]),
            cache: Mutex::new(VecDeque::new()),
            jobs: tokio::sync::Semaphore::new(2),
            capabilities: tokio::sync::OnceCell::new(),
        }
    }
}
fn failed() -> AppError {
    bad_request("Transcoding failed. Check FFmpeg, encoder drivers/device permissions, and input format. Ask the source owner to check the configured encoder if hardware encoding fails.")
}
fn missing() -> AppError {
    AppError(
        StatusCode::NOT_FOUND,
        "Playback session expired or media access was revoked. Select transcoding again.".into(),
    )
}
fn busy() -> AppError {
    AppError(
        StatusCode::TOO_MANY_REQUESTS,
        "Transcoding is busy. Try again shortly.".into(),
    )
}
fn demux(path: &FsPath) -> Result<&'static str, AppError> {
    match path
        .extension()
        .and_then(|v| v.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "mkv" | "webm" => Ok("matroska"),
        "mp4" | "m4v" | "mov" => Ok("mov"),
        "avi" => Ok("avi"),
        "mpeg" | "mpg" => Ok("mpeg"),
        "ts" => Ok("mpegts"),
        _ => Err(bad_request(
            "This input container is not supported for transcoding.",
        )),
    }
}
fn fingerprint(path: &FsPath) -> Result<String, AppError> {
    let m = std::fs::metadata(path).map_err(|_| missing())?;
    Ok(format!(
        "{}:{}:{:?}",
        path.display(),
        m.len(),
        m.modified().map_err(|_| missing())?
    ))
}
fn vaapi() -> String {
    std::env::var("GRIMOIRE_VAAPI_DEVICE").unwrap_or_else(|_| "/dev/dri/renderD128".into())
}
fn encoder_args(encoder: Encoder, height: u32) -> Vec<String> {
    let filter=format!("scale=-2:'min({height},ih)':force_original_aspect_ratio=decrease:force_divisible_by=2,format=yuv420p");
    let mut args = vec![
        "-vf".into(),
        if encoder == Encoder::Amd {
            format!("{filter},format=nv12,hwupload")
        } else {
            filter
        },
        "-c:v".into(),
        match encoder {
            Encoder::Nvidia => "h264_nvenc",
            Encoder::Amd => "h264_vaapi",
            Encoder::Amf => "h264_amf",
            _ => "libx264",
        }
        .into(),
    ];
    if encoder == Encoder::Cpu {
        args.extend(["-preset", "veryfast"].map(String::from));
    }
    if encoder == Encoder::Nvidia {
        args.extend(["-preset", "p4"].map(String::from));
    }
    args.extend(
        [
            "-profile:v",
            "main",
            "-level:v",
            if height > 1080 { "5.1" } else { "4.1" },
            "-b:v",
            &format!("{}k", bitrate(height)),
            "-maxrate",
            &format!("{}k", bitrate(height) * 12 / 10),
            "-bufsize",
            &format!("{}k", bitrate(height) * 2),
            "-r",
            "30",
            "-g",
            "180",
            "-bf",
            "0",
            "-threads",
            "2",
        ]
        .map(String::from),
    );
    args
}
async fn execute(
    mut command: tokio::process::Command,
    limit: u64,
    timeout: u64,
) -> Result<Vec<u8>, AppError> {
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child=command.spawn().map_err(|_|bad_request("The API could not start FFmpeg/FFprobe. Check its executable PATH and runtime libraries."))?;
    let result = tokio::time::timeout(Duration::from_secs(timeout), async {
        let mut bytes = vec![];
        child
            .stdout
            .take()
            .ok_or_else(failed)?
            .take(limit + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| failed())?;
        if bytes.len() as u64 > limit {
            return Err(bad_request(
                "Transcoding output exceeds the segment resource limit.",
            ));
        }
        if !child.wait().await.map_err(|_| failed())?.success() {
            return Err(failed());
        }
        Ok(bytes)
    })
    .await;
    match result {
        Ok(Ok(v)) => Ok(v),
        value => {
            let _ = child.kill().await;
            match value {
                Ok(Err(e)) => Err(e),
                _ => Err(bad_request(
                    "Transcoding timed out. Try a lower resolution or ask the source owner to check its encoder.",
                )),
            }
        }
    }
}
async fn capabilities(runtime: &Runtime) -> Vec<Capability> {
    runtime
        .capabilities
        .get_or_init(|| async {
            let mut result = vec![];
            for encoder in [Encoder::Cpu, Encoder::Nvidia, Encoder::Amd, Encoder::Amf] {
                let mut command = tokio::process::Command::new("ffmpeg");
                command.args(["-v", "error", "-nostdin"]);
                if encoder == Encoder::Amd {
                    command.args(["-vaapi_device", &vaapi()]);
                }
                command
                    .args(["-f", "lavfi", "-i", "color=c=black:s=320x180:r=30:d=0.1"])
                    .args(encoder_args(encoder, 720))
                    .args(["-frames:v", "2", "-f", "mpegts", "pipe:1"]);
                result.push(Capability {
                    encoder,
                    available: execute(command, 1024 * 1024, 15).await.is_ok(),
                });
            }
            result
        })
        .await
        .clone()
}
pub async fn status(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<Response, AppError> {
    let owner: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE owner_id=$1)")
        .bind(identity.user.id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    if !owner {
        return Err(missing());
    }
    json_response(capabilities(&state.transcode).await)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Start {
    source_id: String,
    path: String,
    height: u32,
    audio: Option<u32>,
}
#[derive(Serialize)]
pub struct Started {
    id: String,
    encoder: Encoder,
    height: u32,
    bitrate_kbps: u32,
    audio_bitrate_kbps: u32,
}
async fn probe(path: &FsPath) -> Result<serde_json::Value, AppError> {
    let mut cmd = tokio::process::Command::new("ffprobe");
    cmd.args([
        "-v",
        "error",
        "-protocol_whitelist",
        "file,pipe",
        "-f",
        demux(path)?,
        "-i",
    ])
    .arg(path)
    .args([
        "-show_entries",
        "format=duration:stream=index,codec_type,height:stream_disposition=default",
        "-of",
        "json",
    ]);
    serde_json::from_slice(&execute(cmd, 256 * 1024, 20).await?).map_err(|_| failed())
}
pub async fn start(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(body): Json<Start>,
) -> Result<Response, AppError> {
    if ![0, 480, 720, 1080, 1440, 2160].contains(&body.height)
        || body.path.len() > 4096
        || body.source_id.len() > 128
    {
        return Err(bad_request("Invalid transcoding settings."));
    }
    let root = crate::media_root(&state, &identity.user, &body.source_id).await?;
    let path = crate::resolve_media_path(&root, &body.path, "video")?;
    auth::rate_limit(
        &state.db,
        format!("transcode-start:{}", identity.user.id),
        30,
        300,
    )
    .await?;
    let policy = source_policy(&state, &body.source_id).await?;
    let available = if policy == Encoder::Cpu {
        vec![Capability {
            encoder: Encoder::Cpu,
            available: true,
        }]
    } else {
        capabilities(&state.transcode).await
    };
    let encoder = if policy == Encoder::Auto {
        [Encoder::Nvidia, Encoder::Amd, Encoder::Amf, Encoder::Cpu]
            .into_iter()
            .find(|e| available.iter().any(|c| c.encoder == *e && c.available))
            .ok_or_else(failed)?
    } else {
        policy
    };
    if !available
        .iter()
        .any(|c| c.encoder == encoder && c.available)
    {
        return Err(bad_request(
            "The source provider encoder is unavailable. Ask the source owner to check its drivers/device permissions or choose CPU.",
        ));
    }
    let _job = state.transcode.jobs.try_acquire().map_err(|_| busy())?;
    let value = probe(&path).await?;
    let streams = value["streams"].as_array().ok_or_else(failed)?;
    if streams.len() > 128 || !streams.iter().any(|s| s["codec_type"] == "video") {
        return Err(failed());
    }
    let source_height = streams
        .iter()
        .find(|s| s["codec_type"] == "video")
        .and_then(|s| s["height"].as_u64())
        .filter(|h| *h >= 2)
        .ok_or_else(failed)? as u32;
    let height = if body.height == 0 {
        source_height.min(2160)
    } else {
        body.height.min(source_height).min(2160)
    };
    let audio = if let Some(index) = body.audio {
        if !streams
            .iter()
            .any(|s| s["codec_type"] == "audio" && s["index"].as_u64() == Some(index as u64))
        {
            return Err(bad_request("Audio track not found."));
        }
        Some(index)
    } else {
        streams
            .iter()
            .filter(|s| s["codec_type"] == "audio")
            .max_by_key(|s| s["disposition"]["default"].as_u64().unwrap_or(0))
            .and_then(|s| s["index"].as_u64())
            .map(|v| v as u32)
    };
    let duration = value["format"]["duration"]
        .as_str()
        .and_then(|v| v.parse::<f64>().ok())
        .filter(|v| v.is_finite() && *v > 0. && *v <= 86400.)
        .ok_or_else(failed)?;
    crate::media_root(&state, &identity.user, &body.source_id).await?;
    if source_policy(&state, &body.source_id).await? != policy {
        return Err(missing());
    }
    let session = Session {
        id: uuid::Uuid::new_v4().to_string(),
        user: identity.user.id,
        source: body.source_id,
        path: body.path,
        fingerprint: fingerprint(&path)?,
        encoder,
        height,
        policy,
        audio,
        duration,
        touched: Instant::now(),
    };
    let response = Started {
        id: session.id.clone(),
        encoder,
        height,
        bitrate_kbps: bitrate(height),
        audio_bitrate_kbps: if audio.is_some() { 192 } else { 0 },
    };
    let mut sessions = state.transcode.sessions.lock().unwrap();
    sessions.retain(|s| s.touched.elapsed() < Duration::from_secs(1800));
    // Bound concurrent sessions without invalidating another tab or device.
    if sessions.len() >= 32
        || sessions
            .iter()
            .filter(|s| s.user == identity.user.id)
            .count()
            >= 4
    {
        return Err(busy());
    }
    sessions.push(session);
    json_response(response)
}
async fn session(
    state: &AppState,
    user: &auth::User,
    id: &str,
) -> Result<(Session, PathBuf), AppError> {
    let session = {
        let mut sessions = state.transcode.sessions.lock().unwrap();
        sessions.retain(|s| s.touched.elapsed() < Duration::from_secs(1800));
        let value = sessions
            .iter_mut()
            .find(|s| s.id == id && s.user == user.id)
            .ok_or_else(missing)?;
        value.touched = Instant::now();
        value.clone()
    };
    let root = crate::media_root(state, user, &session.source).await?;
    let path = crate::resolve_media_path(&root, &session.path, "video")?;
    if source_policy(state, &session.source).await? != session.policy
        || fingerprint(&path)? != session.fingerprint
    {
        return Err(missing());
    }
    Ok((session, path))
}
fn playlist(session: &Session) -> String {
    let mut text="#EXTM3U\n#EXT-X-VERSION:3\n#EXT-X-TARGETDURATION:6\n#EXT-X-MEDIA-SEQUENCE:0\n#EXT-X-PLAYLIST-TYPE:VOD\n#EXT-X-INDEPENDENT-SEGMENTS\n".to_string();
    for index in 0..(session.duration / SECONDS).ceil() as u32 {
        text += &format!(
            "#EXTINF:{:.6},\n/api/transcode/{}/segment?index={}\n",
            (session.duration - index as f64 * SECONDS).min(SECONDS),
            session.id,
            index
        );
    }
    text + "#EXT-X-ENDLIST\n"
}
fn response(bytes: impl Into<axum::body::Body>, content: &str) -> Response {
    let mut response = Response::new(bytes.into());
    response
        .headers_mut()
        .insert("content-type", content.parse().unwrap());
    response
        .headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response
}
pub async fn manifest(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<String>,
) -> Result<Response, AppError> {
    let (s, _) = session(&state, &identity.user, &id).await?;
    Ok(response(playlist(&s), "application/vnd.apple.mpegurl"))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SegmentQuery {
    index: u32,
}
async fn encode(
    path: &FsPath,
    s: &Session,
    index: u32,
    encoder: Encoder,
) -> Result<Vec<u8>, AppError> {
    let offset = index as f64 * SECONDS;
    let mut command = tokio::process::Command::new("ffmpeg");
    command.args([
        "-v",
        "error",
        "-nostdin",
        "-max_alloc",
        "268435456",
        "-filter_threads",
        "2",
    ]);
    if encoder == Encoder::Amd {
        command.args(["-vaapi_device", &vaapi()]);
    }
    command
        .args([
            "-ss",
            &offset.to_string(),
            "-protocol_whitelist",
            "file,pipe",
            "-f",
            demux(path)?,
            "-i",
        ])
        .arg(path)
        .args([
            "-t",
            &(s.duration - offset).min(SECONDS).to_string(),
            "-map",
            "0:v:0",
        ]);
    if let Some(audio) = s.audio {
        command.args([
            "-map",
            &format!("0:{audio}"),
            "-c:a",
            "aac",
            "-b:a",
            "192k",
            "-ac",
            "2",
            "-ar",
            "48000",
        ]);
    } else {
        command.arg("-an");
    }
    command.args(encoder_args(encoder, s.height)).args([
        "-sn",
        "-dn",
        "-map_metadata",
        "-1",
        "-map_chapters",
        "-1",
        "-output_ts_offset",
        &offset.to_string(),
        "-mpegts_flags",
        "+resend_headers",
        "-f",
        "mpegts",
        "pipe:1",
    ]);
    execute(command, 24 * 1024 * 1024, 90).await
}
pub async fn segment(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<String>,
    Query(query): Query<SegmentQuery>,
) -> Result<Response, AppError> {
    let (s, path) = session(&state, &identity.user, &id).await?;
    if query.index as f64 * SECONDS >= s.duration {
        return Err(missing());
    }
    auth::rate_limit(
        &state.db,
        format!("transcode-segment:{}", identity.user.id),
        600,
        300,
    )
    .await?;
    let cached = {
        state
            .transcode
            .cache
            .lock()
            .unwrap()
            .iter()
            .find(|(key, index, _)| key == &id && *index == query.index)
            .map(|(_, _, bytes)| bytes.clone())
    };
    if let Some(bytes) = cached {
        return Ok(response(bytes, "video/mp2t"));
    }
    let _permit = state.transcode.jobs.try_acquire().map_err(|_| busy())?;
    let data = encode(&path, &s, query.index, s.encoder).await?;
    session(&state, &identity.user, &id).await?;
    let bytes = Bytes::from(data);
    let mut cache = state.transcode.cache.lock().unwrap();
    while cache.len() >= 8 {
        cache.pop_front();
    }
    cache.push_back((id, query.index, bytes.clone()));
    Ok(response(bytes, "video/mp2t"))
}
pub async fn stop(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<String>,
) -> StatusCode {
    state
        .transcode
        .sessions
        .lock()
        .unwrap()
        .retain(|s| !(s.id == id && s.user == identity.user.id));
    state
        .transcode
        .cache
        .lock()
        .unwrap()
        .retain(|(key, _, _)| key != &id);
    StatusCode::NO_CONTENT
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn flags_are_fixed_and_hardware_specific() {
        assert!(encoder_args(Encoder::Nvidia, 720).contains(&"h264_nvenc".into()));
        assert!(encoder_args(Encoder::Amd, 1080)
            .iter()
            .any(|v| v.contains("hwupload")));
        assert!(encoder_args(Encoder::Cpu, 720).contains(&"libx264".into()));
        assert!(demux(FsPath::new("playlist.m3u8")).is_err());
    }
}

fn bitrate(height: u32) -> u32 {
    match height {
        0..=480 => 1000,
        481..=720 => 2500,
        721..=1080 => 5000,
        1081..=1440 => 10000,
        _ => 20000,
    }
}
async fn source_policy(state: &AppState, source: &str) -> Result<Encoder, AppError> {
    let value: String = sqlx::query_scalar("SELECT transcode_encoder FROM sources WHERE id=$1")
        .bind(source)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| auth::unavailable())?
        .ok_or_else(missing)?;
    serde_json::from_value(serde_json::Value::String(value)).map_err(|_| failed())
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    encoder: Encoder,
}
pub async fn get_policy(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
) -> Result<Response, AppError> {
    let owner: bool = sqlx::query_scalar("SELECT owner_id=$2 FROM sources WHERE id=$1")
        .bind(&source)
        .bind(identity.user.id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| auth::unavailable())?
        .ok_or_else(missing)?;
    if !owner {
        return Err(missing());
    }
    json_response(PolicyView {
        encoder: source_policy(&state, &source).await?,
    })
}
#[derive(Serialize)]
pub struct PolicyView {
    encoder: Encoder,
}
pub async fn set_policy(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(body): Json<Policy>,
) -> Result<StatusCode, AppError> {
    let value = serde_json::to_value(body.encoder).map_err(|_| failed())?;
    let changed =
        sqlx::query("UPDATE sources SET transcode_encoder=$3 WHERE id=$1 AND owner_id=$2")
            .bind(&source)
            .bind(identity.user.id)
            .bind(value.as_str().unwrap())
            .execute(&state.db)
            .await
            .map_err(|_| auth::unavailable())?
            .rows_affected();
    if changed == 0 {
        return Err(missing());
    }
    Ok(StatusCode::NO_CONTENT)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QualityQuery {
    source_id: String,
    path: String,
}
#[derive(Serialize)]
pub struct Quality {
    height: u32,
    bitrate_kbps: u32,
}
pub async fn qualities(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(query): Query<QualityQuery>,
) -> Result<Response, AppError> {
    let root = crate::media_root(&state, &identity.user, &query.source_id).await?;
    let path = crate::resolve_media_path(&root, &query.path, "video")?;
    auth::rate_limit(
        &state.db,
        format!("transcode-quality:{}", identity.user.id),
        60,
        300,
    )
    .await?;
    let _permit = state.transcode.jobs.try_acquire().map_err(|_| busy())?;
    let value = probe(&path).await?;
    let height = value["streams"]
        .as_array()
        .and_then(|s| s.iter().find(|s| s["codec_type"] == "video"))
        .and_then(|v| v["height"].as_u64())
        .filter(|v| *v >= 2)
        .ok_or_else(failed)? as u32;
    let max = height.min(2160);
    let mut heights: Vec<_> = [480, 720, 1080, 1440, 2160]
        .into_iter()
        .filter(|h| *h < max)
        .collect();
    heights.push(max);
    crate::media_root(&state, &identity.user, &query.source_id).await?;
    json_response(
        heights
            .into_iter()
            .map(|height| Quality {
                height,
                bitrate_kbps: bitrate(height),
            })
            .collect::<Vec<_>>(),
    )
}

fn json_response(value: impl Serialize) -> Result<Response, AppError> {
    Ok(response(
        serde_json::to_vec(&value).map_err(|_| auth::unavailable())?,
        "application/json",
    ))
}
