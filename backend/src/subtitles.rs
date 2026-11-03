//! Bounded, authenticated sidecar captions. No remote URLs or embedded styling.
use crate::{auth, AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::io::AsyncReadExt;
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ListQuery {
    path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReadQuery {
    video: String,
    path: String,
    #[serde(default)]
    preview: bool,
}
#[derive(Serialize)]
pub struct Track {
    path: String,
    label: String,
    format: String,
}
fn denied() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Video or caption not found.".into())
}
async fn video(
    state: &AppState,
    user: &auth::User,
    source: &str,
    path: &str,
) -> Result<(PathBuf, PathBuf), AppError> {
    if path.len() > 4096 {
        return Err(denied());
    }
    let root = crate::media_root(state, user, source).await?;
    let file = crate::resolve_media_path(&root, path, "video")?;
    Ok((root, file))
}
fn companion(video: &std::path::Path, subtitle: &std::path::Path) -> bool {
    if video.parent() != subtitle.parent() {
        return false;
    }
    let Some(video) = video.file_stem().and_then(|name| name.to_str()) else {
        return false;
    };
    let Some(caption) = subtitle.file_stem().and_then(|name| name.to_str()) else {
        return false;
    };
    caption == video
        || caption
            .strip_prefix(video)
            .is_some_and(|tail| tail.starts_with('.'))
}
pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<ListQuery>,
) -> Result<Json<serde_json::Value>, AppError> {
    let (root, file) = video(&state, &identity.user, &source, &query.path).await?;
    let embedded_file = file.clone();
    let mut result = tokio::task::spawn_blocking(move || {
        let entries = std::fs::read_dir(file.parent().ok_or_else(denied)?).map_err(|_| denied())?;
        let mut tracks = Vec::new();
        let mut truncated = false;
        for (index, entry) in entries.enumerate() {
            if index >= 5000 {
                truncated = true;
                break;
            }
            let Ok(entry) = entry else { continue };
            let path = entry.path();
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if !matches!(extension.as_str(), "srt" | "vtt" | "ass" | "ssa")
                || !companion(&file, &path)
            {
                continue;
            }
            let Some(relative) = path.strip_prefix(&root).ok().and_then(|path| path.to_str())
            else {
                continue;
            };
            if crate::resolve_media_path(&root, relative, "subtitle").is_err() {
                continue;
            }
            if tracks.len() >= 32 {
                truncated = true;
                continue;
            }
            tracks.push(Track {
                format: if matches!(extension.as_str(), "ass" | "ssa") {
                    "ass"
                } else {
                    "vtt"
                }
                .into(),
                path: relative.into(),
                label: entry.file_name().to_string_lossy().into(),
            });
        }
        tracks.sort_by(|a, b| a.label.cmp(&b.label));
        Ok::<_, AppError>((tracks, truncated))
    })
    .await
    .map_err(|_| auth::unavailable())??;
    let mut fonts = Vec::new();
    let mut warning = None;
    if is_mkv(&embedded_file) {
        auth::rate_limit(
            &state.db,
            format!("subtitle-probe:{}", identity.user.id),
            30,
            60,
        )
        .await?;
        match crate::subtitles_mkv::streams(&embedded_file).await {
            Ok(streams) => for stream in streams {
                if let Some(index) = stream["index"].as_u64() {
                    if let Some(format) = crate::subtitles_mkv::format(&stream) {
                        if result.0.len() >= 32 { result.1 = true; continue; }
                        let language = stream["tags"]["language"].as_str().unwrap_or("und");
                        let title = stream["tags"]["title"].as_str().unwrap_or("Embedded subtitles");
                        let forced = if stream["disposition"]["forced"] == 1 { " · forced" } else { "" };
                        result.0.push(Track { path: format!("embedded:{index}"), label: format!("{title} · {language} · {format}{forced}"), format: format.into() });
                    } else if crate::subtitles_mkv::font(&stream) && fonts.len() < 16 {
                        fonts.push(index);
                    } else if stream["codec_type"] == "attachment" {
                            warning = Some("Some embedded attachments exceed font limits or use unsupported formats; missing fonts use a fallback.");
                        } else if stream["codec_type"] == "subtitle" {
                        warning = Some("Some embedded bitmap/unsupported subtitle tracks cannot be rendered yet.");
                    }
                }
            },
            Err(_) => warning = Some("Embedded tracks could not be inspected. The file may be unsupported, processing may be busy, or the API process may not be able to run FFprobe."),
        }
    }
    crate::media_root(&state, &identity.user, &source).await?;
    Ok(Json(
        serde_json::json!({"tracks":result.0,"truncated":result.1,"fonts":fonts,"warning":warning}),
    ))
}
fn is_mkv(file: &std::path::Path) -> bool {
    file.extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("mkv"))
}

fn timestamp(value: &str) -> Option<u64> {
    if value.len() > 32 {
        return None;
    }
    let value = value.replace(',', ".");
    let (clock, fraction) = value.split_once('.')?;
    if fraction.len() != 3 || !fraction.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let parts: Vec<_> = clock.split(':').collect();
    if !(2..=3).contains(&parts.len())
        || parts
            .iter()
            .any(|part| part.is_empty() || !part.bytes().all(|byte| byte.is_ascii_digit()))
    {
        return None;
    }
    let numbers: Vec<u64> = parts
        .into_iter()
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    let seconds = *numbers.last()?;
    let minutes = numbers[numbers.len() - 2];
    let hours = if numbers.len() == 3 { numbers[0] } else { 0 };
    if hours > 8760 || minutes > 59 || seconds > 59 {
        return None;
    }
    Some(((hours * 60 + minutes) * 60 + seconds) * 1000 + fraction.parse::<u64>().ok()?)
}
fn clock(ms: u64) -> String {
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        ms / 3600000,
        ms / 60000 % 60,
        ms / 1000 % 60,
        ms % 1000
    )
}
fn convert(input: &str) -> Result<String, AppError> {
    let invalid = || {
        crate::bad_request("The caption file is invalid or unsupported. Use UTF-8 SRT or WebVTT.")
    };
    if input.len() > 1024 * 1024 || input.contains('\0') {
        return Err(invalid());
    }
    let input = input
        .trim_start_matches('\u{feff}')
        .replace("\r\n", "\n")
        .replace('\r', "\n");
    let mut output = String::from("WEBVTT\n\n");
    let mut count = 0;
    for block in input.split("\n\n") {
        let lines: Vec<_> = block.trim_matches('\n').lines().collect();
        if lines.is_empty()
            || lines[0].starts_with("WEBVTT")
            || lines[0].starts_with("NOTE")
            || lines[0].starts_with("STYLE")
            || lines[0].starts_with("REGION")
        {
            continue;
        }
        let Some((index, timing)) = lines
            .iter()
            .enumerate()
            .take(2)
            .find(|(_, line)| line.contains("-->"))
        else {
            return Err(invalid());
        };
        let (start, end) = timing.split_once("-->").ok_or_else(invalid)?;
        let start = timestamp(start.trim()).ok_or_else(invalid)?;
        let end =
            timestamp(end.split_whitespace().next().ok_or_else(invalid)?).ok_or_else(invalid)?;
        if end <= start {
            return Err(invalid());
        }
        count += 1;
        if count > 10000 {
            return Err(invalid());
        }
        output.push_str(&format!("{} --> {}\n", clock(start), clock(end)));
        for line in &lines[index + 1..] {
            output.push_str(
                &line
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;"),
            );
            output.push('\n');
        }
        output.push('\n');
        if output.len() > 4 * 1024 * 1024 {
            return Err(invalid());
        }
    }
    if count == 0 {
        return Err(invalid());
    }
    Ok(output)
}
pub async fn read(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<ReadQuery>,
) -> Result<Response, AppError> {
    let (root, file) = video(&state, &identity.user, &source, &query.video).await?;
    if query.path.len() > 4096 {
        return Err(denied());
    }
    if let Some(index) = query.path.strip_prefix("embedded:") {
        let index: u64 = index.parse().map_err(|_| denied())?;
        if !is_mkv(&file) {
            return Err(denied());
        }
        auth::rate_limit(
            &state.db,
            format!("subtitle-extract:{}", identity.user.id),
            20,
            60,
        )
        .await?;
        let streams = crate::subtitles_mkv::streams(&file).await?;
        let stream = streams
            .iter()
            .find(|stream| stream["index"].as_u64() == Some(index))
            .ok_or_else(denied)?;
        let format = crate::subtitles_mkv::format(stream).ok_or_else(denied)?;
        let bytes = if query.preview {
            if format != "ass" {
                return Err(crate::bad_request(
                    "Preview is only available for embedded ASS tracks.",
                ));
            }
            crate::subtitles_mkv::preview(&file, index).await?
        } else {
            crate::subtitles_mkv::extract(&file, index, false, format).await?
        };
        let content = std::str::from_utf8(&bytes).map_err(|_| denied())?;
        let output = if format == "ass" {
            validate_ass(content)?.to_owned()
        } else {
            convert(content)?
        };
        crate::media_root(&state, &identity.user, &source).await?;
        return Ok((
            [
                (
                    "content-type",
                    if format == "ass" {
                        "text/plain; charset=utf-8"
                    } else {
                        "text/vtt; charset=utf-8"
                    },
                ),
                ("cache-control", "private, no-store"),
                ("x-content-type-options", "nosniff"),
            ],
            output,
        )
            .into_response());
    }
    let caption = crate::resolve_media_path(&root, &query.path, "subtitle")?;
    if !companion(&file, &caption) {
        return Err(denied());
    }
    let ass = matches!(caption_extension(&query.path).as_str(), "ass" | "ssa");
    let limit = crate::subtitles_mkv::script_limit(if ass { "ass" } else { "vtt" });
    let input = tokio::fs::File::open(caption).await.map_err(|_| denied())?;
    let mut bytes = Vec::new();
    input
        .take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| denied())?;
    if bytes.len() > limit {
        return Err(crate::bad_request(if ass {
            "ASS/SSA scripts must be at most 16 MiB."
        } else {
            "SRT/WebVTT files must be at most 1 MiB."
        }));
    }
    let content = std::str::from_utf8(&bytes)
        .map_err(|_| crate::bad_request("Caption files must use UTF-8."))?;
    let output = if ass {
        validate_ass(content)?.to_owned()
    } else {
        convert(content)?
    };
    crate::media_root(&state, &identity.user, &source).await?;
    Ok((
        [
            (
                "content-type",
                if ass {
                    "text/plain; charset=utf-8"
                } else {
                    "text/vtt; charset=utf-8"
                },
            ),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        output,
    )
        .into_response())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn normalizes_srt_and_drops_styles_and_active_markup() {
        let output = convert("1\r\n00:00:01,500 --> 00:00:03,000\r\n<script>alert(1)</script>\r\n")
            .unwrap_or_default();
        assert!(output.contains("00:00:01.500 --> 00:00:03.000"));
        assert!(!output.contains("<script>"));
        assert!(output.contains("&lt;script&gt;"));
        let vtt = convert(
            "WEBVTT\n\nSTYLE\n::cue { color:red }\n\n00:01.000 --> 00:02.000 align:start\nHi\n",
        )
        .unwrap_or_default();
        assert!(!vtt.contains("STYLE"));
        assert!(!vtt.contains("align"));
        assert!(vtt.contains("Hi"));
    }
    #[test]
    fn rejects_bad_timing_and_unrelated_files() {
        assert!(convert("1\n00:60:00,000 --> 00:61:00,000\nHi").is_err());
        assert!(convert("00:02.000 --> 00:01.000\nHi").is_err());
        assert!(convert("not a subtitle").is_err());
        assert!(companion(
            std::path::Path::new("a/movie.mp4"),
            std::path::Path::new("a/movie.en.srt")
        ));
        assert!(!companion(
            std::path::Path::new("a/movie.mp4"),
            std::path::Path::new("b/movie.srt")
        ));
        assert!(!companion(
            std::path::Path::new("a/movie.mp4"),
            std::path::Path::new("a/movie-other.srt")
        ));
    }
}

fn caption_extension(path: &str) -> String {
    path.rsplit('.').next().unwrap_or("").to_ascii_lowercase()
}
fn validate_ass(content: &str) -> Result<&str, AppError> {
    if content.len() > crate::subtitles_mkv::ASS_LIMIT
        || content.contains('\0')
        || !content.contains("[Script Info]")
        || !content.contains("[Events]")
    {
        return Err(crate::bad_request(
            "Invalid ASS/SSA script or script exceeds the 16 MiB limit.",
        ));
    }
    Ok(content)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FontQuery {
    video: String,
    index: u64,
}
pub async fn read_font(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<FontQuery>,
) -> Result<Response, AppError> {
    let (_, file) = video(&state, &identity.user, &source, &query.video).await?;
    if !is_mkv(&file) {
        return Err(denied());
    }
    auth::rate_limit(
        &state.db,
        format!("subtitle-font:{}", identity.user.id),
        40,
        60,
    )
    .await?;
    let streams = crate::subtitles_mkv::streams(&file).await?;
    if !streams.iter().any(|stream| {
        stream["index"].as_u64() == Some(query.index) && crate::subtitles_mkv::font(stream)
    }) {
        return Err(denied());
    }
    let bytes = crate::subtitles_mkv::extract(&file, query.index, true, "").await?;
    crate::media_root(&state, &identity.user, &source).await?;
    Ok((
        [
            ("content-type", "application/octet-stream"),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

#[cfg(test)]
mod ass_limits_tests {
    #[test]
    fn typesetting_heavy_ass_uses_its_own_bound() {
        let script = format!("[Script Info]\n[Events]\n{}", "x".repeat(2 * 1024 * 1024));
        assert!(super::validate_ass(&script).is_ok());
        assert!(super::convert(&script).is_err());
        let oversized = format!(
            "[Script Info]\n[Events]\n{}",
            "x".repeat(crate::subtitles_mkv::ASS_LIMIT)
        );
        assert!(super::validate_ass(&oversized).is_err());
    }
}
