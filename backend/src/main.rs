use axum::{
    extract::{Path as RoutePath, State},
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
use tower_http::trace::TraceLayer;

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
