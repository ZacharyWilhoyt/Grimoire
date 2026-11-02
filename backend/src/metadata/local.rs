use super::model::{self, Fields};
use crate::{bad_request, AppError};
use image::{codecs::jpeg::JpegEncoder, ImageReader};
use regex::Regex;
use serde_json::Value;
use std::{
    io::{Cursor, Read},
    path::Path,
    sync::LazyLock,
    time::Duration,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub struct Extracted {
    pub fields: Fields,
    pub artwork: Option<Vec<u8>>,
    pub warnings: Vec<String>,
}
fn failed() -> AppError {
    bad_request("The local metadata could not be read.")
}
pub fn jpeg(bytes: &[u8]) -> Result<Vec<u8>, AppError> {
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(failed());
    }
    let mut reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| failed())?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader
        .decode()
        .map_err(|_| failed())?
        .thumbnail(600, 600)
        .to_rgb8();
    let mut output = Vec::new();
    JpegEncoder::new_with_quality(&mut output, 75)
        .encode_image(&image)
        .map_err(|_| failed())?;
    if output.len() > 524288 {
        return Err(failed());
    }
    Ok(output)
}
// Media parsers receive only bounded bytes through stdin. Nested file/network
// protocols are unavailable; neither filenames nor metadata become shell code.
async fn tool(
    path: &Path,
    program: &str,
    args: &[&str],
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    let input = tokio::fs::File::open(path).await.map_err(|_| failed())?;
    let mut child = tokio::process::Command::new(program)
        .args(args)
        .kill_on_drop(true)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|_| failed())?;
    let mut stdin = child.stdin.take().ok_or_else(failed)?;
    let writer = tokio::spawn(async move {
        let _ = tokio::io::copy(&mut input.take(64 * 1024 * 1024), &mut stdin).await;
        let _ = stdin.shutdown().await;
    });
    let result = tokio::time::timeout(Duration::from_secs(20), async {
        let stdout = child.stdout.take().ok_or_else(failed)?;
        let mut bytes = Vec::new();
        stdout
            .take(limit as u64 + 1)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| failed())?;
        if bytes.len() > limit {
            return Err(failed());
        }
        if !child.wait().await.map_err(|_| failed())?.success() {
            return Err(failed());
        }
        Ok(bytes)
    })
    .await;
    writer.abort();
    match result {
        Ok(Ok(bytes)) => Ok(bytes),
        _ => {
            let _ = child.kill().await;
            Err(failed())
        }
    }
}
static EPISODE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?i)^(.*?)[ ._-]+s(\d{1,3})[ ._-]*e(\d{1,4})(?:[ ._-]|$)")
        .expect("episode expression")
});
pub fn filename_fields(path: &str) -> Fields {
    let mut fields = model::fallback(path);
    if let Some(captures) = EPISODE.captures(&model::filename_title(path)) {
        fields.collection = Some(captures[1].replace('.', " ").trim().to_string());
        fields.season = captures[2].parse().ok();
        fields.episode = captures[3].parse().ok();
    }
    fields
}
fn number(value: &str) -> Option<u32> {
    value
        .split('/')
        .next()?
        .trim()
        .parse::<u32>()
        .ok()
        .filter(|value| *value <= 100_000)
}
pub fn probe_fields(json: &Value, base: Fields) -> Fields {
    let mut tags = std::collections::HashMap::new();
    for object in json
        .get("streams")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|stream| stream.get("tags").and_then(Value::as_object))
        .chain(
            json.get("format")
                .and_then(|format| format.get("tags"))
                .and_then(Value::as_object),
        )
    {
        for (key, value) in object {
            if let Some(value) = value.as_str() {
                tags.insert(key.to_lowercase(), value.to_string());
            }
        }
    }
    let get = |keys: &[&str], limit| {
        keys.iter()
            .find_map(|key| tags.get(*key))
            .map(|value| model::text(value, limit))
    };
    let fields = Fields {
        title: get(&["title"], 300),
        creator: get(&["album_artist", "albumartist", "artist", "author"], 300),
        collection: get(&["album", "show", "series"], 300),
        year: get(&["date", "year"], 4).and_then(|value| value.parse().ok()),
        track: get(&["track"], 30).and_then(|value| number(&value)),
        disc: get(&["disc"], 30).and_then(|value| number(&value)),
        season: get(&["season_number"], 30).and_then(|value| number(&value)),
        episode: get(&["episode_sort", "episode_id"], 30).and_then(|value| number(&value)),
        genres: get(&["genre"], 1200).map(|value| {
            value
                .split([';', ','])
                .take(20)
                .map(|value| model::text(value, 60))
                .collect()
        }),
        description: get(&["description", "comment"], 4000),
        ..Fields::default()
    };
    let mut result = base;
    result.overlay(fields);
    result
}
fn zip_read<R: Read + std::io::Seek>(
    archive: &mut zip::ZipArchive<R>,
    name: &str,
    limit: u64,
) -> Result<Vec<u8>, AppError> {
    let entry = archive.by_name(name).map_err(|_| failed())?;
    if entry.size() > limit {
        return Err(failed());
    }
    let mut bytes = Vec::new();
    entry
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| failed())?;
    if bytes.len() as u64 > limit {
        return Err(failed());
    }
    Ok(bytes)
}
fn xml(bytes: &[u8]) -> Result<roxmltree::Document<'_>, AppError> {
    roxmltree::Document::parse(std::str::from_utf8(bytes).map_err(|_| failed())?)
        .map_err(|_| failed())
}
fn entry_path(base: &str, reference: &str) -> Option<String> {
    let base = reqwest::Url::parse(&format!("https://archive.invalid/{base}")).ok()?;
    let url = base.join(reference).ok()?;
    if url.host_str() != Some("archive.invalid") || url.scheme() != "https" {
        return None;
    }
    Some(
        percent_encoding::percent_decode_str(url.path().trim_start_matches('/'))
            .decode_utf8()
            .ok()?
            .into(),
    )
}
fn book(path: &Path) -> Result<Extracted, AppError> {
    let file = std::fs::File::open(path).map_err(|_| failed())?;
    if file.metadata().map_err(|_| failed())?.len() > 128 * 1024 * 1024 {
        return Err(failed());
    }
    let mut archive = zip::ZipArchive::new(file).map_err(|_| failed())?;
    if archive.len() > 5000 {
        return Err(failed());
    }
    let mut fields = model::fallback(path.to_str().unwrap_or("Book"));
    let cover = if path
        .extension()
        .and_then(|value| value.to_str())
        .is_some_and(|value| value.eq_ignore_ascii_case("epub"))
    {
        let container = zip_read(&mut archive, "META-INF/container.xml", 1024 * 1024)?;
        let doc = xml(&container)?;
        let package = doc
            .descendants()
            .find(|node| node.has_tag_name("rootfile"))
            .and_then(|node| node.attribute("full-path"))
            .ok_or_else(failed)?
            .to_string();
        let bytes = zip_read(&mut archive, &package, 4 * 1024 * 1024)?;
        let doc = xml(&bytes)?;
        let value = |name| {
            doc.descendants()
                .find(|node| node.is_element() && node.tag_name().name() == name)
                .and_then(|node| node.text())
                .map(|value| model::text(value, 300))
        };
        fields.title = value("title").or(fields.title);
        fields.creator = value("creator");
        fields.year =
            value("date").and_then(|value| value.chars().take(4).collect::<String>().parse().ok());
        fields.genres = Some(
            doc.descendants()
                .filter(|node| node.is_element() && node.tag_name().name() == "subject")
                .filter_map(|node| node.text())
                .take(20)
                .map(|value| model::text(value, 60))
                .collect(),
        );
        fields.description = doc
            .descendants()
            .find(|node| node.is_element() && node.tag_name().name() == "description")
            .and_then(|node| node.text())
            .map(|value| model::text(value, 4000));
        fields.collection = doc
            .descendants()
            .find(|node| {
                node.attribute("property") == Some("belongs-to-collection")
                    || node.attribute("name") == Some("calibre:series")
            })
            .and_then(|node| node.attribute("content").or_else(|| node.text()))
            .map(|value| model::text(value, 300));
        fields.volume = doc
            .descendants()
            .find(|node| {
                node.attribute("property") == Some("group-position")
                    || node.attribute("name") == Some("calibre:series_index")
            })
            .and_then(|node| node.attribute("content").or_else(|| node.text()))
            .and_then(|value| value.parse().ok());
        let cover_id = doc
            .descendants()
            .find(|node| node.attribute("name") == Some("cover"))
            .and_then(|node| node.attribute("content"));
        doc.descendants()
            .find(|node| {
                node.tag_name().name() == "item"
                    && (node.attribute("properties").is_some_and(|props| {
                        props.split_whitespace().any(|prop| prop == "cover-image")
                    }) || cover_id.is_some_and(|id| node.attribute("id") == Some(id)))
            })
            .and_then(|node| node.attribute("href"))
            .and_then(|reference| entry_path(&package, reference))
    } else {
        let info = archive
            .file_names()
            .find(|name| {
                name.rsplit('/')
                    .next()
                    .is_some_and(|name| name.eq_ignore_ascii_case("ComicInfo.xml"))
            })
            .map(str::to_string);
        if let Some(info) = info {
            let bytes = zip_read(&mut archive, &info, 1024 * 1024)?;
            let doc = xml(&bytes)?;
            let value = |name| {
                doc.descendants()
                    .find(|node| node.has_tag_name(name))
                    .and_then(|node| node.text())
                    .map(|value| model::text(value, 300))
            };
            fields.title = value("Title")
                .filter(|value| !value.is_empty())
                .or(fields.title);
            fields.creator = value("Writer");
            fields.collection = value("Series");
            fields.year = value("Year").and_then(|value| value.parse().ok());
            fields.volume = value("Number").and_then(|value| value.parse().ok());
            fields.genres = value("Genre").map(|value| {
                value
                    .split(',')
                    .take(20)
                    .map(|value| model::text(value, 60))
                    .collect()
            });
            fields.description = doc
                .descendants()
                .find(|node| node.has_tag_name("Summary"))
                .and_then(|node| node.text())
                .map(|value| model::text(value, 4000));
        }
        let mut names: Vec<_> = archive
            .file_names()
            .filter(|name| {
                ["jpg", "jpeg", "png", "webp", "gif"]
                    .iter()
                    .any(|ext| name.to_lowercase().ends_with(&format!(".{ext}")))
            })
            .map(str::to_string)
            .collect();
        names.sort_by(|a, b| natord::compare(a, b));
        names.into_iter().next()
    };
    let artwork = cover
        .and_then(|name| zip_read(&mut archive, &name, 8 * 1024 * 1024).ok())
        .and_then(|bytes| jpeg(&bytes).ok());
    Ok(Extracted {
        fields: sanitize(fields).normalize()?,
        artwork,
        warnings: Vec::new(),
    })
}
fn sanitize(mut fields: Fields) -> Fields {
    fields.year = fields.year.filter(|year| (1..=9999).contains(year));
    fields.volume = fields
        .volume
        .filter(|number| number.is_finite() && (0.0..=100_000.0).contains(number));
    fields
}
pub async fn extract(path: &Path, kind: &str) -> Result<Extracted, AppError> {
    if kind == "book" {
        let path = path.to_owned();
        return tokio::task::spawn_blocking(move || book(&path))
            .await
            .map_err(|_| failed())?;
    }
    let mut result = Extracted {
        fields: filename_fields(path.to_str().unwrap_or("Media")),
        artwork: None,
        warnings: Vec::new(),
    };
    let args=["-v","error","-protocol_whitelist","pipe","-max_alloc","33554432","-probesize","5000000","-analyzeduration","2000000","-i","pipe:0","-show_entries","format_tags=title,artist,album,album_artist,albumartist,author,date,year,genre,track,disc,show,series,season_number,episode_sort,episode_id,description,comment:stream_tags=title,artist,album,genre,track,disc","-of","json"];
    match tool(path, "ffprobe", &args, 262144)
        .await
        .and_then(|bytes| serde_json::from_slice::<Value>(&bytes).map_err(|_| failed()))
    {
        Ok(json) => result.fields = probe_fields(&json, result.fields),
        Err(_) => result
            .warnings
            .push("Embedded audio/video tags were unavailable; filename metadata was used.".into()),
    }
    let args = [
        "-v",
        "error",
        "-protocol_whitelist",
        "pipe",
        "-max_alloc",
        "33554432",
        "-threads",
        "1",
        "-filter_threads",
        "1",
        "-i",
        "pipe:0",
        "-map",
        "0:v:0",
        "-an",
        "-vf",
        "scale=600:600:force_original_aspect_ratio=decrease",
        "-frames:v",
        "1",
        "-c:v",
        "mjpeg",
        "-f",
        "image2pipe",
        "pipe:1",
    ];
    if let Ok(bytes) = tool(path, "ffmpeg", &args, 2 * 1024 * 1024).await {
        result.artwork = tokio::task::spawn_blocking(move || jpeg(&bytes))
            .await
            .ok()
            .and_then(Result::ok);
    }
    result.fields = sanitize(result.fields).normalize()?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn tags_and_episode_names() {
        let fields=probe_fields(&serde_json::json!({"format":{"tags":{"TITLE":"Track","ARTIST":"Artist","ALBUM":"Album","TRACK":"2/10","GENRE":"Rock;Ambient","DATE":"2024-02-01"}}}),Fields::default()).normalize().unwrap_or_else(|_|panic!("tags"));
        assert_eq!(fields.track, Some(2));
        assert_eq!(fields.year, Some(2024));
        assert_eq!(fields.genres, Some(vec!["rock".into(), "ambient".into()]));
        let episode = filename_fields("Example.Show.S02E03.1080p.mkv");
        assert_eq!(episode.collection.as_deref(), Some("Example Show"));
        assert_eq!(episode.season, Some(2));
        assert_eq!(episode.episode, Some(3));
    }
    #[test]
    fn comic_metadata_and_natural_cover_order() {
        use std::io::Write;
        let path =
            std::env::temp_dir().join(format!("grimoire-comic-{}.cbz", uuid::Uuid::new_v4()));
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (name, color) in [("page10.jpg", [240, 20, 20]), ("page2.jpg", [20, 240, 20])] {
            let mut bytes = Vec::new();
            JpegEncoder::new(&mut bytes)
                .encode_image(&image::RgbImage::from_pixel(10, 10, image::Rgb(color)))
                .unwrap();
            archive
                .start_file(name, zip::write::SimpleFileOptions::default())
                .unwrap();
            archive.write_all(&bytes).unwrap();
        }
        archive
            .start_file("ComicInfo.xml", zip::write::SimpleFileOptions::default())
            .unwrap();
        archive.write_all(b"<ComicInfo><Title>A Comic</Title><Series>Comics</Series><Writer>A Writer</Writer><Number>2.5</Number><Genre>Fantasy, Adventure</Genre></ComicInfo>").unwrap();
        archive.finish().unwrap();
        let result = book(&path).unwrap_or_else(|_| panic!("comic metadata"));
        assert_eq!(result.fields.collection.as_deref(), Some("Comics"));
        assert_eq!(result.fields.volume, Some(2.5));
        assert_eq!(
            result.fields.genres,
            Some(vec!["fantasy".into(), "adventure".into()])
        );
        let cover = image::load_from_memory(result.artwork.as_ref().unwrap())
            .unwrap()
            .to_rgb8();
        assert!(cover.get_pixel(0, 0)[1] > cover.get_pixel(0, 0)[0]);
        std::fs::remove_file(path).unwrap();
        assert!(xml(
            b"<!DOCTYPE test [<!ENTITY secret SYSTEM 'file:///etc/passwd'>]><test>&secret;</test>"
        )
        .is_err());
    }
    #[test]
    fn archive_paths_and_artwork() {
        assert_eq!(
            entry_path("OPS/book.opf", "images/cover.png").as_deref(),
            Some("OPS/images/cover.png")
        );
        assert!(entry_path("OPS/book.opf", "https://attacker.test/image").is_none());
        assert!(jpeg(b"<svg onload='alert(1)'/>").is_err());
    }
}
