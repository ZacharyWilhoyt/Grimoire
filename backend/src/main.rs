use axum::{
    extract::{Path as RoutePath, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use std::{
    fs, io,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
use tokio::sync::RwLock;
use tower_http::{services::ServeFile, trace::TraceLayer};

const MAX_MEDIA_FILES: usize = 10_000;
const MAX_DIRECTORY_ENTRIES: usize = 100_000;
const MAX_DIRECTORY_DEPTH: usize = 32;

#[derive(Clone)]
struct AppState {
    sources: Arc<RwLock<Vec<Source>>>,
    data_dir: Arc<PathBuf>,
    next_source_id: Arc<AtomicU64>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Source {
    id: String,
    name: String,
    path: PathBuf,
}

#[derive(Serialize)]
struct SourceSummary {
    id: String,
    name: String,
    available: bool,
}

#[derive(Deserialize)]
struct AddSourceRequest {
    path: String,
}

#[derive(Serialize)]
struct MediaFile {
    name: String,
    relative_path: String,
    kind: &'static str,
    extension: String,
    size_bytes: u64,
}

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct ApiError {
    error: String,
}

struct AppError(StatusCode, String);

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        (self.0, Json(ApiError { error: self.1 })).into_response()
    }
}

fn bad_request(message: impl Into<String>) -> AppError {
    AppError(StatusCode::BAD_REQUEST, message.into())
}

fn internal_error() -> AppError {
    AppError(
        StatusCode::INTERNAL_SERVER_ERROR,
        "The source registry could not be saved.".into(),
    )
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "grimoire-api",
    })
}

async fn list_sources(State(state): State<AppState>) -> Json<Vec<SourceSummary>> {
    let sources = state.sources.read().await;
    Json(
        sources
            .iter()
            .map(|source| SourceSummary {
                id: source.id.clone(),
                name: source.name.clone(),
                available: source.path.is_dir(),
            })
            .collect(),
    )
}

async fn add_source(
    State(state): State<AppState>,
    Json(request): Json<AddSourceRequest>,
) -> Result<(StatusCode, Json<SourceSummary>), AppError> {
    let input = request.path.trim();
    if input.is_empty() || input.len() > 4096 {
        return Err(bad_request(
            "Enter a directory path up to 4096 characters long.",
        ));
    }

    let canonical_path = fs::canonicalize(input)
        .map_err(|_| bad_request("That directory could not be found or opened."))?;
    if !canonical_path.is_dir() {
        return Err(bad_request("The selected path is not a directory."));
    }
    if canonical_path.to_str().is_none() {
        return Err(bad_request("Directory paths must use valid UTF-8."));
    }

    let mut sources = state.sources.write().await;
    if sources.iter().any(|source| source.path == canonical_path) {
        return Err(AppError(
            StatusCode::CONFLICT,
            "That directory is already connected.".into(),
        ));
    }

    let source_id = state.next_source_id.fetch_add(1, Ordering::Relaxed);
    let name = canonical_path
        .file_name()
        .and_then(|part| part.to_str())
        .filter(|part| !part.is_empty())
        .unwrap_or("Local directory")
        .to_string();
    let source = Source {
        id: format!("source-{source_id}"),
        name: name.clone(),
        path: canonical_path,
    };

    let mut updated = sources.clone();
    updated.push(source.clone());
    persist_sources(&state.data_dir, &updated)
        .await
        .map_err(|_| internal_error())?;
    *sources = updated;

    Ok((
        StatusCode::CREATED,
        Json(SourceSummary {
            id: source.id,
            name,
            available: true,
        }),
    ))
}

async fn remove_source(
    State(state): State<AppState>,
    RoutePath(source_id): RoutePath<String>,
) -> Result<StatusCode, AppError> {
    let mut sources = state.sources.write().await;
    if !sources.iter().any(|source| source.id == source_id) {
        return Err(AppError(StatusCode::NOT_FOUND, "Source not found.".into()));
    }
    let updated: Vec<_> = sources
        .iter()
        .filter(|source| source.id != source_id)
        .cloned()
        .collect();
    persist_sources(&state.data_dir, &updated)
        .await
        .map_err(|_| internal_error())?;
    *sources = updated;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_media(
    State(state): State<AppState>,
    RoutePath(source_id): RoutePath<String>,
) -> Result<Json<Vec<MediaFile>>, AppError> {
    let root = {
        let sources = state.sources.read().await;
        sources
            .iter()
            .find(|source| source.id == source_id)
            .map(|source| source.path.clone())
            .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Source not found.".into()))?
    };

    if !root.is_dir() {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "This directory is currently unavailable.".into(),
        ));
    }

    let files = tokio::task::spawn_blocking(move || scan_directory(&root))
        .await
        .map_err(|_| internal_error())?
        .map_err(|message| AppError(StatusCode::INTERNAL_SERVER_ERROR, message))?;

    Ok(Json(files))
}

#[derive(Deserialize)]
struct MediaRequest {
    path: String,
}

// This boundary currently trusts the single local operator. Keep loopback binding
// until real identity and source/media authorization are implemented.
async fn stream_audio(
    State(state): State<AppState>,
    RoutePath(source_id): RoutePath<String>,
    Query(query): Query<MediaRequest>,
    request: Request,
) -> Result<Response, AppError> {
    stream_media(state, source_id, query, request, "music").await
}

async fn stream_video(
    State(state): State<AppState>,
    RoutePath(source_id): RoutePath<String>,
    Query(query): Query<MediaRequest>,
    request: Request,
) -> Result<Response, AppError> {
    stream_media(state, source_id, query, request, "video").await
}

async fn stream_book(
    State(state): State<AppState>,
    RoutePath(source_id): RoutePath<String>,
    Query(query): Query<MediaRequest>,
    request: Request,
) -> Result<Response, AppError> {
    stream_media(state, source_id, query, request, "book").await
}

async fn stream_media(
    state: AppState,
    source_id: String,
    query: MediaRequest,
    request: Request,
    kind: &str,
) -> Result<Response, AppError> {
    let sources = state.sources.read().await;
    let root = sources
        .iter()
        .find(|source| source.id == source_id)
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Source not found.".into()))?
        .path
        .clone();
    let path = resolve_media_path(&root, &query.path, kind)?;
    if kind == "book"
        && fs::metadata(&path)
            .map_err(|_| bad_request("Book unavailable."))?
            .len()
            > 128 * 1024 * 1024
    {
        return Err(AppError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Books must be smaller than 128 MiB.".into(),
        ));
    }
    let response = ServeFile::new(path)
        .try_call(request)
        .await
        .map_err(|_| AppError(StatusCode::NOT_FOUND, "Media file is unavailable.".into()))?;
    let mut response = response.map(axum::body::Body::new);
    response
        .headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    if kind == "book" {
        response
            .headers_mut()
            .insert("content-disposition", "attachment".parse().unwrap());
    }
    Ok(response)
}

fn resolve_media_path(root: &Path, relative: &str, kind: &str) -> Result<PathBuf, AppError> {
    let denied = || AppError(StatusCode::NOT_FOUND, "Media file not found.".into());
    let relative = Path::new(relative);
    if relative.as_os_str().is_empty()
        || relative
            .components()
            .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err(denied());
    }
    // Reject symlinks at every component, matching the directory scanner.
    let mut path = root.to_path_buf();
    for component in relative.components() {
        path.push(component);
        if fs::symlink_metadata(&path)
            .map_err(|_| denied())?
            .file_type()
            .is_symlink()
        {
            return Err(denied());
        }
    }
    let canonical = fs::canonicalize(&path).map_err(|_| denied())?;
    if !canonical.starts_with(root) || !canonical.is_file() {
        return Err(denied());
    }
    let extension = canonical
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if classify_extension(&extension) != Some(kind) {
        return Err(denied());
    }
    Ok(canonical)
}

fn classify_extension(extension: &str) -> Option<&'static str> {
    match extension {
        "mkv" | "mp4" | "m4v" | "mov" | "avi" | "webm" | "mpeg" | "mpg" | "ts" => Some("video"),
        "mp3" | "flac" | "m4a" | "aac" | "ogg" | "opus" | "wav" | "aiff" | "wma" => Some("music"),
        "epub" | "cbz" => Some("book"),
        _ => None,
    }
}

fn scan_directory(root: &Path) -> Result<Vec<MediaFile>, String> {
    let mut files = Vec::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    let mut visited_entries = 0usize;

    'scan: while let Some((directory, depth)) = pending.pop() {
        let entries = match fs::read_dir(&directory) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries {
            visited_entries += 1;
            if visited_entries >= MAX_DIRECTORY_ENTRIES || files.len() >= MAX_MEDIA_FILES {
                break 'scan;
            }

            let entry = match entry {
                Ok(entry) => entry,
                Err(_) => continue,
            };
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(_) => continue,
            };

            // Do not follow symlinks, which could make a source expose files outside its root.
            if file_type.is_symlink() {
                continue;
            }
            let path = entry.path();
            if file_type.is_dir() {
                if depth < MAX_DIRECTORY_DEPTH {
                    pending.push((path, depth + 1));
                }
                continue;
            }
            if !file_type.is_file() {
                continue;
            }

            let extension = match path.extension().and_then(|part| part.to_str()) {
                Some(extension) => extension.to_ascii_lowercase(),
                None => continue,
            };
            let kind = match classify_extension(&extension) {
                Some(kind) => kind,
                None => continue,
            };
            let relative_path = match path.strip_prefix(root).ok().and_then(Path::to_str) {
                Some(path) => path.to_string(),
                None => continue,
            };
            let metadata = match entry.metadata() {
                Ok(metadata) => metadata,
                Err(_) => continue,
            };
            let file_name = entry.file_name();
            let name = match file_name.to_str() {
                Some(name) => name.to_string(),
                None => continue,
            };

            files.push(MediaFile {
                name,
                relative_path,
                kind,
                extension,
                size_bytes: metadata.len(),
            });
        }
    }

    files.sort_by(|left, right| {
        left.relative_path
            .to_lowercase()
            .cmp(&right.relative_path.to_lowercase())
    });
    Ok(files)
}

async fn persist_sources(data_dir: &Path, sources: &[Source]) -> io::Result<()> {
    tokio::fs::create_dir_all(data_dir).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(data_dir, fs::Permissions::from_mode(0o700)).await?;
    }
    let json = serde_json::to_vec_pretty(sources).map_err(io::Error::other)?;
    let temporary_file = data_dir.join("sources.json.tmp");
    let registry_file = data_dir.join("sources.json");
    tokio::fs::write(&temporary_file, json).await?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        tokio::fs::set_permissions(&temporary_file, fs::Permissions::from_mode(0o600)).await?;
    }
    tokio::fs::rename(temporary_file, registry_file).await
}

async fn not_found() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "not found".into(),
        }),
    )
}

#[tokio::main]
async fn main() {
    let data_dir = std::env::var_os("GRIMOIRE_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));
    let registry_file = data_dir.join("sources.json");
    let existing_sources: Vec<Source> = match tokio::fs::read(&registry_file).await {
        Ok(contents) => serde_json::from_slice(&contents)
            .expect("source registry is invalid; repair it before restarting Grimoire"),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => panic!("could not read source registry: {error}"),
    };
    let next_id = existing_sources
        .iter()
        .filter_map(|source| source.id.strip_prefix("source-")?.parse::<u64>().ok())
        .max()
        .unwrap_or(0)
        + 1;

    let state = AppState {
        sources: Arc::new(RwLock::new(existing_sources)),
        data_dir: Arc::new(data_dir),
        next_source_id: Arc::new(AtomicU64::new(next_id)),
    };
    let app = Router::new()
        .route("/api/health", get(health))
        .route("/api/sources", get(list_sources).post(add_source))
        .route("/api/sources/{source_id}", delete(remove_source))
        .route("/api/sources/{source_id}/media", get(list_media))
        .route("/api/sources/{source_id}/audio", get(stream_audio))
        .route("/api/sources/{source_id}/video", get(stream_video))
        .route("/api/sources/{source_id}/book", get(stream_book))
        .fallback(not_found)
        .with_state(state)
        .layer(TraceLayer::new_for_http());

    let address = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind API listener");
    println!("Grimoire API listening on http://{address}");

    axum::serve(listener, app).await.expect("API server failed");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "grimoire-audio-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("track.mp3"), b"0123456789").unwrap();
        fs::write(root.join("story.epub"), b"0123456789").unwrap();
        fs::write(root.join("comic.cbz"), b"0123456789").unwrap();
        fs::write(root.join("clip.mp4"), b"0123456789").unwrap();
        fs::write(root.join("private.txt"), b"private").unwrap();
        fs::canonicalize(root).unwrap()
    }

    #[test]
    fn audio_paths_deny_traversal_non_audio_and_symlinks() {
        let root = fixture();
        assert!(resolve_media_path(&root, "track.mp3", "music").is_ok());
        for path in [
            "../track.mp3",
            "/etc/passwd",
            "private.txt",
            "missing.mp3",
            "",
        ] {
            assert!(
                resolve_media_path(&root, path, "music").is_err(),
                "accepted {path}"
            );
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("track.mp3"), root.join("link.mp3")).unwrap();
            assert!(resolve_media_path(&root, "link.mp3", "music").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn video_paths_require_video_and_deny_escapes() {
        let root = fixture();
        assert!(resolve_media_path(&root, "clip.mp4", "video").is_ok());
        for path in [
            "track.mp3",
            "private.txt",
            "../clip.mp4",
            "/etc/passwd",
            "missing.mp4",
        ] {
            assert!(resolve_media_path(&root, path, "video").is_err());
        }
        assert!(resolve_media_path(&root, "clip.mp4", "music").is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(root.join("clip.mp4"), root.join("linked.mp4")).unwrap();
            assert!(resolve_media_path(&root, "linked.mp4", "video").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn audio_supports_ranges_and_requires_registered_source() {
        let root = fixture();
        let state = AppState {
            sources: Arc::new(RwLock::new(vec![Source {
                id: "local".into(),
                name: "Local".into(),
                path: root.clone(),
            }])),
            data_dir: Arc::new(root.clone()),
            next_source_id: Arc::new(AtomicU64::new(2)),
        };
        let request = Request::builder()
            .header("range", "bytes=2-4")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = stream_audio(
            State(state.clone()),
            RoutePath("local".into()),
            Query(MediaRequest {
                path: "track.mp3".into(),
            }),
            request,
        )
        .await
        .unwrap_or_else(|_| panic!("stream failed"));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-range"], "bytes 2-4/10");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 10)
                .await
                .unwrap()
                .as_ref(),
            b"234"
        );
        state.sources.write().await.clear();
        let result = stream_audio(
            State(state),
            RoutePath("local".into()),
            Query(MediaRequest {
                path: "track.mp3".into(),
            }),
            Request::new(axum::body::Body::empty()),
        )
        .await;
        assert!(matches!(result, Err(AppError(StatusCode::NOT_FOUND, _))));
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn book_paths_require_books_and_deny_escapes() {
        let root = fixture();
        for path in ["story.epub", "comic.cbz"] {
            assert!(resolve_media_path(&root, path, "book").is_ok());
        }
        for path in ["track.mp3", "clip.mp4", "../story.epub", "/etc/passwd"] {
            assert!(resolve_media_path(&root, path, "book").is_err());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[tokio::test]
    async fn video_supports_ranges_and_requires_registered_source() {
        let root = fixture();
        let state = AppState {
            sources: Arc::new(RwLock::new(vec![Source {
                id: "local".into(),
                name: "Local".into(),
                path: root.clone(),
            }])),
            data_dir: Arc::new(root.clone()),
            next_source_id: Arc::new(AtomicU64::new(2)),
        };
        let request = Request::builder()
            .header("range", "bytes=2-4")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = stream_video(
            State(state.clone()),
            RoutePath("local".into()),
            Query(MediaRequest {
                path: "clip.mp4".into(),
            }),
            request,
        )
        .await
        .unwrap_or_else(|_| panic!("stream failed"));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-range"], "bytes 2-4/10");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 10)
                .await
                .unwrap()
                .as_ref(),
            b"234"
        );
        state.sources.write().await.clear();
        let result = stream_video(
            State(state),
            RoutePath("local".into()),
            Query(MediaRequest {
                path: "clip.mp4".into(),
            }),
            Request::new(axum::body::Body::empty()),
        )
        .await;
        assert!(matches!(result, Err(AppError(StatusCode::NOT_FOUND, _))));
        fs::remove_dir_all(root).unwrap();
    }
    #[tokio::test]
    async fn book_supports_ranges_and_requires_registered_source() {
        let root = fixture();
        let state = AppState {
            sources: Arc::new(RwLock::new(vec![Source {
                id: "local".into(),
                name: "Local".into(),
                path: root.clone(),
            }])),
            data_dir: Arc::new(root.clone()),
            next_source_id: Arc::new(AtomicU64::new(2)),
        };
        let request = Request::builder()
            .header("range", "bytes=2-4")
            .body(axum::body::Body::empty())
            .unwrap();
        let response = stream_book(
            State(state.clone()),
            RoutePath("local".into()),
            Query(MediaRequest {
                path: "story.epub".into(),
            }),
            request,
        )
        .await
        .unwrap_or_else(|_| panic!("stream failed"));
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(response.headers()["content-range"], "bytes 2-4/10");
        assert_eq!(response.headers()["cache-control"], "private, no-store");
        assert_eq!(
            axum::body::to_bytes(response.into_body(), 10)
                .await
                .unwrap()
                .as_ref(),
            b"234"
        );
        fs::File::create(root.join("oversized.epub"))
            .unwrap()
            .set_len(129 * 1024 * 1024)
            .unwrap();
        let oversized = stream_book(
            State(state.clone()),
            RoutePath("local".into()),
            Query(MediaRequest {
                path: "oversized.epub".into(),
            }),
            Request::new(axum::body::Body::empty()),
        )
        .await;
        assert!(matches!(
            oversized,
            Err(AppError(StatusCode::PAYLOAD_TOO_LARGE, _))
        ));
        state.sources.write().await.clear();
        let result = stream_book(
            State(state),
            RoutePath("local".into()),
            Query(MediaRequest {
                path: "story.epub".into(),
            }),
            Request::new(axum::body::Body::empty()),
        )
        .await;
        assert!(matches!(result, Err(AppError(StatusCode::NOT_FOUND, _))));
        fs::remove_dir_all(root).unwrap();
    }
}
