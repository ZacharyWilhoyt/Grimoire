use axum::{
    extract::{Path as RoutePath, Query, Request, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{delete, get},
    Json, Router,
};
mod admin;
mod auth;
#[cfg(test)]
mod auth_integration;
mod catalog;
mod metadata;
mod music;
mod permissions;
mod progress;
mod statistics;
mod subtitles;
mod subtitles_mkv;
mod transcode;
mod video_audio;

use serde::{Deserialize, Serialize};
use std::{
    fs,
    future::IntoFuture,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};
use tower_http::{services::ServeFile, trace::TraceLayer};

const MAX_MEDIA_FILES: usize = 10_000;
const MAX_DIRECTORY_ENTRIES: usize = 100_000;
const MAX_DIRECTORY_DEPTH: usize = 32;

#[derive(Clone)]
struct AppState {
    db: sqlx::PgPool,
    config: Arc<auth::Config>,
    hashing: Arc<tokio::sync::Semaphore>,
    catalog_scans: Arc<tokio::sync::Semaphore>,
    metadata: Arc<metadata::Runtime>,
    video_audio: Arc<video_audio::Runtime>,
    transcode: Arc<transcode::Runtime>,
}

#[derive(sqlx::FromRow)]
struct Source {
    id: String,
    name: String,
    path: String,
    is_owner: bool,
    can_access_media: bool,
    acquisition_policy: String,
}

#[derive(Serialize)]
struct SourceSummary {
    id: String,
    name: String,
    available: bool,
    is_owner: bool,
    can_access_media: bool,
    acquisition_policy: String,
}

#[derive(Deserialize)]
struct AddSourceRequest {
    path: String,
}

#[derive(Clone, Serialize)]
struct MediaFile {
    name: String,
    relative_path: String,
    kind: &'static str,
    extension: String,
    size_bytes: u64,
    modified_at: Option<f64>,
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
        let mut response = (self.0, Json(ApiError { error: self.1 })).into_response();
        response
            .headers_mut()
            .insert("cache-control", "private, no-store".parse().unwrap());
        response
    }
}

fn bad_request(message: impl Into<String>) -> AppError {
    AppError(StatusCode::BAD_REQUEST, message.into())
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "grimoire-api",
    })
}

// Probes must not build an unbounded database wait queue during an outage.
static READINESS_PROBES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(2);
async fn readiness(State(state): State<AppState>) -> Response {
    let ready = match READINESS_PROBES.try_acquire() {
        Ok(_permit) => matches!(
            tokio::time::timeout(
                Duration::from_secs(2),
                sqlx::query("SELECT 1").execute(&state.db)
            )
            .await,
            Ok(Ok(_))
        ),
        Err(_) => false,
    };
    let mut response = (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(Health {
            status: if ready { "ready" } else { "not_ready" },
            service: "grimoire-api",
        }),
    )
        .into_response();
    response
        .headers_mut()
        .insert("cache-control", "no-store".parse().unwrap());
    response
}

async fn list_sources(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
) -> Result<Json<Vec<SourceSummary>>, AppError> {
    let sources = sqlx::query_as::<_, Source>(
        "SELECT s.id,s.name,s.path,(s.owner_id=$1) AS is_owner,(s.owner_id=$1 OR COALESCE(p.can_discover AND p.can_access_media,false)) AS can_access_media,COALESCE(p.acquisition_policy,'auto_deny') AS acquisition_policy FROM sources s LEFT JOIN source_permissions p ON p.source_id=s.id AND p.user_id=$1 WHERE s.owner_id=$1 OR p.can_discover ORDER BY s.created_at,s.id",
    )
    .bind(identity.user.id)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    Ok(Json(
        sources
            .into_iter()
            .map(|source| SourceSummary {
                id: source.id,
                name: source.name,
                available: Path::new(&source.path).is_dir(),
                is_owner: source.is_owner,
                can_access_media: source.can_access_media,
                acquisition_policy: source.acquisition_policy,
            })
            .collect(),
    ))
}

async fn add_source(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    Json(request): Json<AddSourceRequest>,
) -> Result<(StatusCode, Json<SourceSummary>), AppError> {
    if !identity.user.is_admin {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Only an administrator may register server directories.".into(),
        ));
    }
    let input = request.path.trim();
    if input.is_empty() || input.len() > 4096 || !Path::new(input).is_absolute() {
        return Err(bad_request(
            "Enter an absolute directory path up to 4096 bytes long.",
        ));
    }
    let input = input.to_string();
    let path = tokio::task::spawn_blocking(move || fs::canonicalize(input))
        .await
        .map_err(|_| auth::unavailable())?
        .map_err(|_| bad_request("That directory could not be found or opened."))?;
    if !path.is_dir() {
        return Err(bad_request("The selected path is not a directory."));
    }
    let path_string = path
        .to_str()
        .ok_or_else(|| bad_request("Directory paths must use valid UTF-8."))?;
    let name = path
        .file_name()
        .and_then(|part| part.to_str())
        .unwrap_or("Local directory")
        .to_string();
    let id = uuid::Uuid::new_v4().to_string();
    let result = sqlx::query("INSERT INTO sources (id,owner_id,name,path) VALUES ($1,$2,$3,$4)")
        .bind(&id)
        .bind(identity.user.id)
        .bind(&name)
        .bind(path_string)
        .execute(&state.db)
        .await;
    match result {
        Ok(_) => Ok((
            StatusCode::CREATED,
            Json(SourceSummary {
                id,
                name,
                available: true,
                is_owner: true,
                can_access_media: true,
                acquisition_policy: "auto_deny".into(),
            }),
        )),
        Err(error)
            if error
                .as_database_error()
                .is_some_and(|e| e.is_unique_violation()) =>
        {
            Err(AppError(
                StatusCode::CONFLICT,
                "That directory is already registered.".into(),
            ))
        }
        Err(_) => Err(auth::unavailable()),
    }
}

async fn media_root(
    state: &AppState,
    user: &auth::User,
    source_id: &str,
) -> Result<PathBuf, AppError> {
    let path: Option<String> = sqlx::query_scalar(
        "SELECT s.path FROM sources s WHERE s.id=$1 AND (s.owner_id=$2 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$2 AND p.can_discover AND p.can_access_media))",
    ).bind(source_id).bind(user.id).fetch_optional(&state.db).await.map_err(|_| auth::unavailable())?;
    path.map(PathBuf::from).ok_or_else(|| {
        AppError(
            StatusCode::NOT_FOUND,
            "Source not found or media access not granted.".into(),
        )
    })
}

async fn remove_source(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    RoutePath(source_id): RoutePath<String>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    let owner: Option<uuid::Uuid> =
        sqlx::query_scalar("SELECT owner_id FROM sources WHERE id=$1 FOR UPDATE")
            .bind(&source_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    if owner != Some(identity.user.id) {
        return Err(AppError(StatusCode::NOT_FOUND, "Source not found.".into()));
    }
    sqlx::query("INSERT INTO source_permission_audit(source_id,actor_id,target_id,action,before_state) SELECT source_id,$2,user_id,'revoked',json_build_object('can_discover',can_discover,'can_access_media',can_access_media,'acquisition_policy',acquisition_policy)::text FROM source_permissions WHERE source_id=$1")
        .bind(&source_id).bind(identity.user.id).execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
    sqlx::query("DELETE FROM sources WHERE id=$1")
        .bind(source_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;

    Ok(StatusCode::NO_CONTENT)
}

async fn list_media(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    RoutePath(source_id): RoutePath<String>,
) -> Result<Json<Vec<catalog::Item>>, AppError> {
    Ok(Json(
        catalog::source_items(&state, &identity.user, &source_id).await?,
    ))
}

#[derive(Deserialize)]
struct MediaRequest {
    path: String,
}

// Identity is resolved server-side for every request, including byte-range requests.
async fn stream_audio(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    RoutePath(source_id): RoutePath<String>,
    Query(query): Query<MediaRequest>,
    request: Request,
) -> Result<Response, AppError> {
    stream_media(state, identity.user, source_id, query, request, "music").await
}

async fn stream_video(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    RoutePath(source_id): RoutePath<String>,
    Query(query): Query<MediaRequest>,
    request: Request,
) -> Result<Response, AppError> {
    stream_media(state, identity.user, source_id, query, request, "video").await
}

async fn stream_book(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    RoutePath(source_id): RoutePath<String>,
    Query(query): Query<MediaRequest>,
    request: Request,
) -> Result<Response, AppError> {
    stream_media(state, identity.user, source_id, query, request, "book").await
}

async fn stream_media(
    state: AppState,
    user: auth::User,
    source_id: String,
    query: MediaRequest,
    request: Request,
    kind: &str,
) -> Result<Response, AppError> {
    let root = media_root(&state, &user, &source_id).await?;
    let path = resolve_media_path(&root, &query.path, kind)?;
    serve_media_file(path, request, kind).await
}

async fn serve_media_file(
    path: PathBuf,
    request: Request,
    kind: &str,
) -> Result<Response, AppError> {
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
    if !(kind == "subtitle" && matches!(extension.as_str(), "srt" | "vtt" | "ass" | "ssa"))
        && classify_extension(&extension) != Some(kind)
    {
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

fn modified_epoch(metadata: &fs::Metadata) -> Option<f64> {
    metadata
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|value| value.as_secs_f64())
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
                modified_at: modified_epoch(&metadata),
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

async fn not_found() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "not found".into(),
        }),
    )
}

fn app(state: AppState) -> Router {
    use axum::{extract::DefaultBodyLimit, middleware};
    let protected = Router::new()
        .route(
            "/api/sources/{source_id}/metadata/group-membership",
            axum::routing::post(metadata::groups::join).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/api/transcode/capabilities", get(transcode::status))
        .route("/api/transcode/qualities", get(transcode::qualities))
        .route(
            "/api/sources/{source_id}/transcoding",
            get(transcode::get_policy).put(transcode::set_policy),
        )
        .route("/api/transcode", axum::routing::post(transcode::start))
        .route(
            "/api/transcode/{id}/manifest.m3u8",
            get(transcode::manifest),
        )
        .route("/api/transcode/{id}/segment", get(transcode::segment))
        .route("/api/transcode/{id}", delete(transcode::stop))
        .route("/api/media/link", get(catalog::linked))
        .route(
            "/api/sources/{source_id}/audio-tracks",
            get(video_audio::list),
        )
        .route(
            "/api/sources/{source_id}/video-audio",
            axum::routing::post(video_audio::prepare),
        )
        .route(
            "/api/sources/{source_id}/video-audio/{rendition}",
            get(video_audio::read),
        )
        .route("/api/sources/{source_id}/subtitles", get(subtitles::list))
        .route("/api/sources/{source_id}/subtitle", get(subtitles::read))
        .route(
            "/api/sources/{source_id}/subtitle-font",
            get(subtitles::read_font),
        )
        .route(
            "/api/progress",
            get(progress::get)
                .put(progress::put)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/groups/{group_id}",
            get(metadata::groups::details),
        )
        .route(
            "/api/sources/{source_id}/groups/{group_id}/genres",
            axum::routing::put(metadata::groups::set_genres)
                .delete(metadata::groups::reset_genres)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/groups/{group_id}/artwork",
            get(metadata::groups::artwork)
                .post(metadata::groups::upload)
                .delete(metadata::groups::reset_artwork)
                .layer(DefaultBodyLimit::max(8 * 1024 * 1024)),
        )
        .route("/api/sources/{source_id}/arr", get(metadata::arr::list))
        .route(
            "/api/sources/{source_id}/arr/{connection_id}",
            axum::routing::put(metadata::arr::save)
                .delete(metadata::arr::remove)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/arr/{connection_id}/test",
            axum::routing::post(metadata::arr::test),
        )
        .route(
            "/api/sources/{source_id}/arr/{connection_id}/preview",
            axum::routing::post(metadata::arr::preview).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/api/metadata/providers", get(metadata::provider_status))
        .route("/api/metadata/artwork/{file_key}", get(metadata::artwork))
        .route(
            "/api/sources/{source_id}/metadata",
            get(metadata::details)
                .put(metadata::edit)
                .delete(metadata::reset)
                .layer(DefaultBodyLimit::max(32 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/metadata/extract",
            axum::routing::post(metadata::extract).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/metadata/search",
            axum::routing::post(metadata::search).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/metadata/preview",
            axum::routing::post(metadata::preview).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/metadata/apply",
            axum::routing::post(metadata::apply).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route(
            "/api/sources/{source_id}/metadata/scan",
            axum::routing::post(metadata::scan),
        )
        .route("/api/sources/{source_id}/metadata/job", get(metadata::job))
        .route(
            "/api/sources/{source_id}/metadata/job/{job_id}/cancel",
            axum::routing::post(metadata::cancel_scan),
        )
        .route("/api/catalog", get(catalog::discover))
        .route("/api/continue", get(catalog::continue_media))
        .route(
            "/api/library",
            get(catalog::library)
                .put(catalog::save)
                .layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/api/library/{file_key}", delete(catalog::unsave))
        .route("/api/activity", get(catalog::recent))
        .route(
            "/api/activity/open",
            axum::routing::post(catalog::opened).layer(DefaultBodyLimit::max(16 * 1024)),
        )
        .route("/api/auth/me", get(auth::me))
        .route("/api/auth/sessions", get(auth::sessions))
        .route("/api/auth/logout", axum::routing::post(auth::logout))
        .route(
            "/api/auth/logout-others",
            axum::routing::post(auth::logout_others),
        )
        .route(
            "/api/auth/password",
            axum::routing::post(auth::change_password),
        )
        .route(
            "/api/music/library",
            get(music::read)
                .put(music::save)
                .layer(DefaultBodyLimit::max(1024 * 1024)),
        )
        .route("/api/statistics", get(statistics::read))
        .route("/api/sources", get(list_sources).post(add_source))
        .route("/api/sources/{source_id}", delete(remove_source))
        .route(
            "/api/sources/{source_id}/permissions",
            get(permissions::list),
        )
        .route(
            "/api/sources/{source_id}/permissions/{username}",
            axum::routing::put(permissions::save).delete(permissions::revoke),
        )
        .route("/api/sources/{source_id}/media", get(list_media))
        .route("/api/sources/{source_id}/audio", get(stream_audio))
        .route("/api/sources/{source_id}/video", get(stream_video))
        .route("/api/sources/{source_id}/book", get(stream_book))
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth::require_session,
        ));
    Router::new()
        .merge(protected)
        .route("/api/health", get(health))
        .route("/api/ready", get(readiness))
        .route("/api/auth/register", axum::routing::post(auth::register))
        .route("/api/auth/login", axum::routing::post(auth::login))
        .fallback(not_found)
        .layer(DefaultBodyLimit::max(4096))
        .layer(middleware::from_fn_with_state(
            state.clone(),
            auth::origin_and_limits,
        ))
        .with_state(state)
        .layer(TraceLayer::new_for_http())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let database_url =
        std::env::var("DATABASE_URL").expect("Set DATABASE_URL to a PostgreSQL connection string.");
    let options: sqlx::postgres::PgConnectOptions = database_url
        .parse()
        .expect("Invalid PostgreSQL connection configuration");
    if !matches!(options.get_host(), "localhost" | "127.0.0.1" | "::1")
        && !options.get_host().starts_with('/')
        && !matches!(
            options.get_ssl_mode(),
            sqlx::postgres::PgSslMode::VerifyFull
        )
    {
        panic!("Remote PostgreSQL connections require sslmode=verify-full.");
    }
    let db = sqlx::postgres::PgPoolOptions::new()
        .max_connections(10)
        .connect_with(options)
        .await
        .expect("PostgreSQL connection failed");
    sqlx::migrate!()
        .run(&db)
        .await
        .expect("Database migration failed");
    let args: Vec<String> = std::env::args().skip(1).collect();
    if !args.is_empty() {
        if let Err(message) = admin::command(&db, &args).await {
            eprintln!("{message}");
            std::process::exit(1);
        }
        return;
    }
    let has_admin: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM users WHERE is_admin AND NOT disabled)")
            .fetch_one(&db)
            .await
            .expect("Administrator lookup failed");
    if !has_admin {
        eprintln!("Create the initial administrator first: cargo run -- bootstrap-admin USERNAME");
        std::process::exit(1);
    }
    let dummy = auth::hash_password(&uuid::Uuid::new_v4().to_string())
        .unwrap_or_else(|_| panic!("Password hashing initialization failed"));
    let config = auth::Config::from_env(dummy).unwrap_or_else(|message| panic!("{message}"));
    let state = AppState {
        db,
        config: Arc::new(config),
        hashing: Arc::new(tokio::sync::Semaphore::new(4)),
        catalog_scans: Arc::new(tokio::sync::Semaphore::new(2)),
        metadata: Arc::new(metadata::Runtime::new()),
        video_audio: Arc::new(crate::video_audio::Runtime::new()),
        transcode: Arc::new(crate::transcode::Runtime::new()),
    };
    let port = std::env::var("GRIMOIRE_PORT")
        .unwrap_or_else(|_| "3000".into())
        .parse::<u16>()
        .expect("GRIMOIRE_PORT must be a valid port");
    let address = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("Failed to bind API listener");
    println!("Grimoire API listening on http://{address}");
    let (shutdown, requested) = tokio::sync::oneshot::channel::<()>();
    let server = axum::serve(
        listener,
        app(state).into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(async {
        let _ = requested.await;
    })
    .into_future();
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result.expect("API server failed"),
        () = shutdown_signal() => {
            eprintln!("Shutdown requested; draining active requests for up to 30 seconds.");
            let _ = shutdown.send(());
            match tokio::time::timeout(Duration::from_secs(30), &mut server).await {
                Ok(result) => result.expect("API server failed during shutdown"),
                Err(_) => eprintln!("Shutdown deadline reached; closing remaining requests."),
            }
        }
    }
}

async fn shutdown_signal() {
    let interrupt = async {
        tokio::signal::ctrl_c()
            .await
            .expect("Could not install interrupt handler");
    };
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Could not install termination handler");
        tokio::select! { () = interrupt => {}, _ = terminate.recv() => {} }
    }
    #[cfg(not(unix))]
    interrupt.await;
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
    async fn media_ranges_and_book_size_limits() {
        let root = fixture();
        for (file, kind) in [
            ("track.mp3", "music"),
            ("clip.mp4", "video"),
            ("story.epub", "book"),
            ("comic.cbz", "book"),
        ] {
            let request = Request::builder()
                .header("range", "bytes=2-4")
                .body(axum::body::Body::empty())
                .unwrap();
            let response = serve_media_file(root.join(file), request, kind)
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
        }
        fs::File::create(root.join("oversized.epub"))
            .unwrap()
            .set_len(129 * 1024 * 1024)
            .unwrap();
        let result = serve_media_file(
            root.join("oversized.epub"),
            Request::new(axum::body::Body::empty()),
            "book",
        )
        .await;
        assert!(matches!(
            result,
            Err(AppError(StatusCode::PAYLOAD_TOO_LARGE, _))
        ));
        fs::remove_dir_all(root).unwrap();
    }
}
