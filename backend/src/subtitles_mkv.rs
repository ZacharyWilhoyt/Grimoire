//! Matroska-only extraction: force the demuxer; never autodetect playlists or URLs.
use crate::{bad_request, AppError};
use serde_json::Value;
use std::{path::Path, process::Stdio, sync::LazyLock, time::Duration};
use tokio::io::AsyncReadExt;
pub const ASS_LIMIT: usize = 16 * 1024 * 1024;
pub fn script_limit(format: &str) -> usize {
    if format == "ass" {
        ASS_LIMIT
    } else {
        1024 * 1024
    }
}
static WAITERS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| tokio::sync::Semaphore::new(8));
static JOBS: LazyLock<tokio::sync::Semaphore> = LazyLock::new(|| tokio::sync::Semaphore::new(2));
fn failed() -> AppError {
    bad_request("Media inspection/extraction failed. The file may be unsupported or exceed processing limits.")
}
async fn run(
    file: &Path,
    probe: bool,
    before: &[String],
    after: &[String],
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    let busy = || {
        crate::AppError(
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            "Media inspection is busy. Try again shortly.".into(),
        )
    };
    let _waiting = WAITERS.try_acquire().map_err(|_| busy())?;
    let _permit = tokio::time::timeout(Duration::from_secs(2), JOBS.acquire())
        .await
        .map_err(|_| busy())?
        .map_err(|_| busy())?;
    let mut command = tokio::process::Command::new(if probe { "ffprobe" } else { "ffmpeg" });
    command
        .args(["-v", "error"])
        .args(before)
        .args(["-protocol_whitelist", "file,pipe", "-f", "matroska", "-i"])
        .arg(file)
        .args(after)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .stdout(Stdio::piped())
        .kill_on_drop(true);
    let mut child = command.spawn().map_err(|_| bad_request("The API process could not start FFmpeg/FFprobe. Check its PATH, executable permissions and runtime libraries."))?;
    let result = tokio::time::timeout(Duration::from_secs(120), async {
        let stdout = child.stdout.take().ok_or_else(failed)?;
        let mut bytes = Vec::new();
        stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| failed())?;
        if bytes.len() > limit {
            return Err(bad_request(format!(
                "Extracted media exceeds its {} MiB output limit.",
                limit / (1024 * 1024)
            )));
        }
        if !child.wait().await.map_err(|_| failed())?.success() {
            return Err(failed());
        }
        Ok(bytes)
    })
    .await;
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        Ok(Err(error)) => {
            let _ = child.kill().await;
            Err(error)
        }
        Err(_) => {
            let _ = child.kill().await;
            Err(bad_request(
                "Media extraction exceeded its 120-second processing limit.",
            ))
        }
    }
}
pub async fn inspect(file: &Path) -> Result<Value, AppError> {
    let bytes = run(file, true, &[], &["-show_entries".into(), "format=duration:stream=index,codec_type,codec_name,extradata_size,channels,duration:stream_tags=language,title,filename,mimetype,DURATION:stream_disposition=default,forced".into(), "-of".into(), "json".into()], 256*1024).await?;
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| failed())?;
    let streams = value["streams"].as_array().ok_or_else(failed)?;
    if streams.len() > 128 {
        return Err(failed());
    }
    Ok(value)
}
pub async fn streams(file: &Path) -> Result<Vec<Value>, AppError> {
    Ok(inspect(file).await?["streams"].as_array().unwrap().clone())
}
pub fn format(stream: &Value) -> Option<&'static str> {
    if stream["codec_type"] != "subtitle" {
        return None;
    }
    match stream["codec_name"].as_str()? {
        "ass" | "ssa" => Some("ass"),
        "subrip" | "webvtt" | "text" => Some("vtt"),
        _ => None,
    }
}
pub fn font(stream: &Value) -> bool {
    stream["codec_type"] == "attachment"
        && (matches!(stream["codec_name"].as_str(), Some("ttf" | "otf"))
            || matches!(
                stream["tags"]["mimetype"].as_str(),
                Some(
                    "font/ttf"
                        | "font/otf"
                        | "application/x-truetype-font"
                        | "application/vnd.ms-opentype"
                        | "application/x-font-ttf"
                        | "application/x-font-opentype"
                )
            ))
        && stream["extradata_size"]
            .as_u64()
            .is_some_and(|size| size > 0 && size <= 8 * 1024 * 1024)
}
pub async fn extract(
    file: &Path,
    index: u64,
    font: bool,
    format: &str,
) -> Result<Vec<u8>, AppError> {
    if font {
        let bytes = run(
            file,
            false,
            &[format!("-dump_attachment:{index}"), "pipe:1".into()],
            &[
                "-map".into(),
                "0:v:0?".into(),
                "-t".into(),
                "0".into(),
                "-f".into(),
                "null".into(),
                "-".into(),
            ],
            8 * 1024 * 1024,
        )
        .await?;
        if !matches!(bytes.get(..4), Some(b"\0\x01\0\0" | b"OTTO" | b"true")) {
            return Err(bad_request(
                "Embedded attachment is not a supported TrueType/OpenType font.",
            ));
        }
        Ok(bytes)
    } else {
        run(
            file,
            false,
            &[],
            &[
                "-map".into(),
                format!("0:{index}"),
                "-c:s".into(),
                if format == "ass" { "copy" } else { "webvtt" }.into(),
                "-f".into(),
                format.into(),
                "pipe:1".into(),
            ],
            script_limit(format),
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn codecs_and_font_limits_are_explicit() {
        assert!(font(
            &serde_json::json!({"codec_type":"attachment","extradata_size":100,"tags":{"mimetype":"font/ttf"}})
        ));
        assert!(!font(
            &serde_json::json!({"codec_type":"attachment","extradata_size":100,"tags":{"mimetype":"image/png","filename":"font.ttf"}})
        ));
        assert_eq!(script_limit("ass"), 16 * 1024 * 1024);
        assert_eq!(script_limit("vtt"), 1024 * 1024);

        assert_eq!(
            format(&serde_json::json!({"codec_type":"subtitle","codec_name":"ass"})),
            Some("ass")
        );
        assert_eq!(
            format(&serde_json::json!({"codec_type":"subtitle","codec_name":"hdmv_pgs_subtitle"})),
            None
        );
        assert!(!font(
            &serde_json::json!({"codec_type":"attachment","codec_name":"ttf","extradata_size":9000000})
        ));
    }
    #[tokio::test]
    #[ignore = "Requires FFmpeg/FFprobe; run with --ignored"]
    async fn embedded_ass_preserves_styles_and_fonts() {
        let root =
            std::env::temp_dir().join(format!("grimoire-subtitles-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        let script = root.join("styled.ass");
        let mut content = include_str!("../tests/fixtures/styled.ass").to_string();
        for _ in 0..1200 {
            content += &format!(
                "Dialogue: 0,0:00:00.00,0:00:01.00,Default,,0,0,0,,{}\n",
                "x".repeat(1024)
            );
        }
        std::fs::write(&script, content).unwrap();
        let font_path = Path::new("/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf");
        let file = root.join("styled.mkv");
        let status = tokio::process::Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-f",
                "lavfi",
                "-i",
                "color=c=black:s=640x360:d=1",
                "-i",
            ])
            .arg(&script)
            .args([
                "-map", "0:v", "-map", "1:s", "-c:v", "libx264", "-threads", "1", "-c:s", "copy",
                "-attach",
            ])
            .arg(font_path)
            .args([
                "-metadata:s:t:0",
                "mimetype=font/ttf",
                "-metadata:s:t:0",
                "filename=DejaVuSans.ttf",
                "-y",
            ])
            .arg(&file)
            .status()
            .await
            .unwrap();
        assert!(status.success());
        let streams = streams(&file)
            .await
            .unwrap_or_else(|_| panic!("probe failed"));
        let subtitle = streams
            .iter()
            .find(|stream| format(stream) == Some("ass"))
            .unwrap();
        let ass = String::from_utf8(
            extract(&file, subtitle["index"].as_u64().unwrap(), false, "ass")
                .await
                .unwrap_or_else(|_| panic!("extraction failed")),
        )
        .unwrap();
        assert!(ass.len() > 1024 * 1024);
        assert!(ass.contains("\\pos(320,30)"));
        assert!(ass.contains("\\k100"));
        assert!(ass.contains("\\p1"));
        assert!(ass.contains("DejaVu Sans"));
        let attachment = streams.iter().find(|stream| font(stream)).unwrap();
        assert_eq!(
            extract(&file, attachment["index"].as_u64().unwrap(), true, "")
                .await
                .unwrap_or_else(|_| panic!("extraction failed")),
            std::fs::read(font_path).unwrap()
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}
