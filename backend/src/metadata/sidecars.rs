//! Local video artwork conventions used by Sonarr's metadata exports.
use super::*;
use std::{
    collections::BTreeMap,
    fs,
    io::Read,
    path::{Path as FsPath, PathBuf},
    sync::LazyLock,
};

const MAX_BYTES: u64 = 8 * 1024 * 1024;
const IMAGE_EXTENSIONS: &[&str] = &["jpg", "jpeg", "png", "webp"];
static IMAGE_WORK: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(4)));

#[derive(Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Thumbnail,
    Banner,
    Poster,
    SeasonPoster,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Thumbnail => "thumbnail",
            Self::Banner => "banner",
            Self::Poster => "poster",
            Self::SeasonPoster => "season_poster",
        }
    }
}

#[derive(Default)]
struct Images {
    banner: Option<PathBuf>,
    poster: Option<PathBuf>,
    season_poster: Option<PathBuf>,
}

fn image(directory: &FsPath, stem: &str) -> Option<PathBuf> {
    IMAGE_EXTENSIONS.iter().find_map(|extension| {
        let file = directory.join(format!("{stem}.{extension}"));
        let metadata = fs::symlink_metadata(&file).ok()?;
        (metadata.is_file() && metadata.len() <= MAX_BYTES).then_some(file)
    })
}

fn thumbnail(video: &FsPath) -> Option<PathBuf> {
    image(
        video.parent()?,
        &format!("{}-thumb", video.file_stem()?.to_str()?),
    )
}

fn series_images(root: &FsPath, video: &FsPath, season: Option<u32>) -> Images {
    let season_stem = season.map(|season| {
        if season == 0 {
            "season-specials-poster".into()
        } else {
            format!("season{season:02}-poster")
        }
    });
    let Some(parent) = video.parent() else {
        return Images::default();
    };
    let mut fallback = None;
    for directory in parent
        .ancestors()
        .take(8)
        .take_while(|path| path.starts_with(root))
    {
        let banner = image(directory, "banner").or_else(|| image(directory, "fanart"));
        let poster = image(directory, "poster");
        let season_poster = season_stem
            .as_deref()
            .and_then(|stem| image(directory, stem))
            .or_else(|| {
                (season == Some(0))
                    .then(|| image(directory, "season00-poster"))
                    .flatten()
            });
        if banner.is_some() || season_poster.is_some() {
            return Images {
                banner,
                poster,
                season_poster,
            };
        }
        if fallback.is_none() {
            fallback = poster;
        }
    }
    Images {
        poster: fallback,
        ..Images::default()
    }
}

fn url(source: &str, path: &str, kind: Kind) -> String {
    let mut url = reqwest::Url::parse("https://grimoire.invalid/").expect("fixed URL");
    url.path_segments_mut()
        .expect("URL path")
        .extend(["api", "sources", source, "video-artwork"]);
    url.query_pairs_mut()
        .append_pair("path", path)
        .append_pair("kind", kind.name());
    format!("{}?{}", url.path(), url.query().unwrap_or(""))
}

pub(super) fn thumbnail_url(source: &str, path: &str, video: &FsPath) -> Option<String> {
    thumbnail(video).map(|_| url(source, path, Kind::Thumbnail))
}

#[derive(Default, Serialize)]
pub struct SeriesArtwork {
    pub banner_url: Option<String>,
    pub poster_url: Option<String>,
}

#[derive(Serialize)]
pub struct Season {
    pub key: String,
    pub count: usize,
    pub poster_url: Option<String>,
}

pub fn season_key(item: &catalog::Item) -> String {
    item.metadata
        .as_ref()
        .and_then(|metadata| metadata.fields.season)
        .map_or_else(|| "unknown".into(), |number| number.to_string())
}

pub fn describe(root: &FsPath, items: &[catalog::Item]) -> (SeriesArtwork, Vec<Season>) {
    let mut artwork = SeriesArtwork::default();
    let mut seasons = BTreeMap::<Option<u32>, Season>::new();
    let mut cached = HashMap::<(PathBuf, Option<u32>), Images>::new();
    for item in items.iter().filter(|item| item.file.kind == "video") {
        let number = item
            .metadata
            .as_ref()
            .and_then(|metadata| metadata.fields.season);
        let season = seasons.entry(number).or_insert_with(|| Season {
            key: season_key(item),
            count: 0,
            poster_url: None,
        });
        season.count += 1;
        if artwork.banner_url.is_some()
            && artwork.poster_url.is_some()
            && season.poster_url.is_some()
        {
            continue;
        }
        let path = &item.file.relative_path;
        let video = root.join(path);
        let Some(directory) = video.parent() else {
            continue;
        };
        let images = cached
            .entry((directory.to_path_buf(), number))
            .or_insert_with(|| series_images(root, &video, number));
        if artwork.banner_url.is_none() && images.banner.is_some() {
            artwork.banner_url = Some(url(&item.source_id, path, Kind::Banner));
        }
        if artwork.poster_url.is_none() && images.poster.is_some() {
            artwork.poster_url = Some(url(&item.source_id, path, Kind::Poster));
        }
        if season.poster_url.is_none() && images.season_poster.is_some() {
            season.poster_url = Some(url(&item.source_id, path, Kind::SeasonPoster));
        }
    }
    let mut seasons: Vec<_> = seasons.into_values().collect();
    // Unknown season files are listed after numbered seasons and specials.
    seasons.sort_by_key(|season| season.key.parse::<u32>().unwrap_or(u32::MAX));
    (artwork, seasons)
}

pub fn attach_thumbnails(root: &FsPath, items: &mut [catalog::Item]) {
    for item in items
        .iter_mut()
        .filter(|item| item.file.kind == "video" && item.available)
    {
        if let Some(metadata) = &mut item.metadata {
            let path = &item.file.relative_path;
            metadata.thumbnail_url = thumbnail_url(&item.source_id, path, &root.join(path));
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QueryArtwork {
    path: String,
    kind: Kind,
}

pub async fn artwork(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<QueryArtwork>,
) -> Result<Response, AppError> {
    let file = reference(&state, &identity.user, &source, &query.path, false).await?;
    if file.kind != "video" {
        return Err(not_found());
    }
    let root = crate::media_root(&state, &identity.user, &source).await?;
    let season = if matches!(query.kind, Kind::Thumbnail) {
        None
    } else {
        let record = load(&state, &file.key).await?;
        view(&query.path, &file.key, record.as_ref(), false, true)
            .fields
            .season
    };
    let permit = IMAGE_WORK
        .clone()
        .acquire_owned()
        .await
        .map_err(|_| auth::unavailable())?;
    let jpeg = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let image = match query.kind {
            Kind::Thumbnail => thumbnail(&file.path),
            kind => {
                let images = series_images(&root, &file.path, season);
                match kind {
                    Kind::Banner => images.banner,
                    Kind::Poster => images.poster,
                    Kind::SeasonPoster => images.season_poster,
                    Kind::Thumbnail => None,
                }
            }
        }
        .ok_or_else(not_found)?;
        let canonical = fs::canonicalize(&image).map_err(|_| not_found())?;
        if canonical != image || !canonical.starts_with(&root) {
            return Err(not_found());
        }
        let input = fs::File::open(canonical).map_err(|_| not_found())?;
        if !input.metadata().map_err(|_| not_found())?.is_file() {
            return Err(not_found());
        }
        let mut bytes = Vec::new();
        input
            .take(MAX_BYTES + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| not_found())?;
        local::jpeg(&bytes).map_err(|_| not_found())
    })
    .await
    .map_err(|_| auth::unavailable())??;
    crate::media_root(&state, &identity.user, &source).await?;
    Ok((
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        jpeg,
    )
        .into_response())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_integration::{body, register_user, request, state};
    use axum::http::Method;

    fn jpeg(path: &FsPath, color: [u8; 3]) {
        image::RgbImage::from_pixel(16, 12, image::Rgb(color))
            .save(path)
            .unwrap();
    }

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "Requires a disposable PostgreSQL database"]
    async fn season_artwork_and_thumbnails_follow_media_access(pool: sqlx::PgPool) {
        let state = state(pool.clone());
        let (owner, _, owner_user) = register_user(&state, "artwork_owner").await;
        let (viewer, _, viewer_user) = register_user(&state, "artwork_viewer").await;
        let root = std::env::temp_dir().join(format!("grimoire-sidecars-{}", Uuid::new_v4()));
        let first = "Blue Box/Season 1/Blue Box - S01E01.mkv";
        let second = "Blue Box/Season 2/Blue Box - S02E01.mkv";
        for file in [first, second] {
            let full = root.join(file);
            fs::create_dir_all(full.parent().unwrap()).unwrap();
            fs::write(full, b"fixture video").unwrap();
        }
        jpeg(&root.join("Blue Box/banner.jpg"), [100, 0, 0]);
        jpeg(&root.join("Blue Box/poster.jpg"), [0, 100, 0]);
        jpeg(&root.join("Blue Box/season02-poster.jpg"), [0, 0, 100]);
        jpeg(
            &root.join("Blue Box/Season 2/Blue Box - S02E01-thumb.jpg"),
            [150, 80, 10],
        );
        #[cfg(unix)]
        std::os::unix::fs::symlink(
            root.join("Blue Box/poster.jpg"),
            root.join("Blue Box/Season 1/Blue Box - S01E01-thumb.jpg"),
        )
        .unwrap();
        let source = Uuid::new_v4().to_string();
        let owner_id = Uuid::parse_str(owner_user["id"].as_str().unwrap()).unwrap();
        let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
        sqlx::query(
            "INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,'Artwork fixture',$3)",
        )
        .bind(&source)
        .bind(owner_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
        let item_url = reqwest::Url::parse_with_params(
            "http://localhost/api/media/link",
            &[("source_id", source.as_str()), ("path", first)],
        )
        .unwrap();
        let item = body(
            request(
                &state,
                Method::GET,
                &format!("{}?{}", item_url.path(), item_url.query().unwrap()),
                Some(&owner),
                None,
                None,
                None,
            )
            .await,
        )
        .await;
        let group = item["item"]["metadata"]["group"]["id"].as_str().unwrap();
        let season_url = format!("/api/media/link?source_id={source}&group={group}&season=2");
        assert_eq!(
            request(
                &state,
                Method::GET,
                &season_url,
                Some(&viewer),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES($1,$2,true,true)")
            .bind(&source).bind(viewer_id).execute(&pool).await.unwrap();
        let season = body(
            request(
                &state,
                Method::GET,
                &season_url,
                Some(&viewer),
                None,
                None,
                None,
            )
            .await,
        )
        .await;
        assert_eq!(season["season"], "2");
        assert_eq!(season["members"].as_array().unwrap().len(), 1);
        assert_eq!(season["members"][0]["relative_path"], second);
        assert_eq!(season["seasons"][0]["key"], "1");
        assert!(season["seasons"][0]["poster_url"].is_null());
        assert_eq!(season["seasons"][1]["key"], "2");
        let mut images = Vec::new();
        for value in [
            &season["artwork"]["banner_url"],
            &season["artwork"]["poster_url"],
            &season["seasons"][1]["poster_url"],
            &season["members"][0]["metadata"]["thumbnail_url"],
        ] {
            let image_url = value.as_str().unwrap();
            let response = request(
                &state,
                Method::GET,
                image_url,
                Some(&viewer),
                None,
                None,
                None,
            )
            .await;
            assert_eq!(response.status(), StatusCode::OK);
            assert_eq!(response.headers()["cache-control"], "private, no-store");
            images.push(
                axum::body::to_bytes(response.into_body(), 524288)
                    .await
                    .unwrap(),
            );
        }
        assert_ne!(images[1], images[2]);
        assert_ne!(images[2], images[3]);
        assert_eq!(
            request(
                &state,
                Method::GET,
                &url(&source, first, Kind::Thumbnail),
                Some(&owner),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        sqlx::query("DELETE FROM source_permissions WHERE source_id=$1 AND user_id=$2")
            .bind(&source)
            .bind(viewer_id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            request(
                &state,
                Method::GET,
                season["seasons"][1]["poster_url"].as_str().unwrap(),
                Some(&viewer),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        fs::remove_dir_all(root).unwrap();
    }
}
