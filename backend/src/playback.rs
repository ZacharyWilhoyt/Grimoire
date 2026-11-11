//! Session-bound selection over existing authorized playback endpoints.
use crate::{auth, catalog, unified_catalog, AppError, AppState, MediaFile};
use axum::{
    extract::{DefaultBodyLimit, Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::post,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::{
    collections::HashMap,
    sync::Mutex,
    time::{Duration, Instant},
};
use uuid::Uuid;

const TTL: Duration = Duration::from_secs(6 * 3600);
const MAX_SESSIONS: usize = 2048;
const MAX_FAILOVERS: usize = 3;
const MAX_OPTIONS: usize = 8;
static LOCAL_PROBES: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(8);

#[derive(Default)]
pub(crate) struct Runtime(Mutex<HashMap<Uuid, Session>>);

#[derive(Clone)]
struct Session {
    auth_session: String,
    owner: Uuid,
    anchor_representation: Uuid,
    logical: Uuid,
    request: Start,
    location: Uuid,
    failed: Vec<Uuid>,
    exact: bool,
    expires: Instant,
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    source_id: String,
    relative_path: String,
    #[serde(default)]
    native_formats: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Switch {
    current: Uuid,
    prefer: Option<Uuid>,
}

#[derive(FromRow)]
struct Location {
    id: Uuid,
    representation_id: Uuid,
    source_id: String,
    source_name: String,
    root: Option<String>,
    path: Option<String>,
    node_id: Option<Uuid>,
    anchor_representation: Uuid,
    logical: Uuid,
    extension: String,
    size_bytes: i64,
    modified_at: Option<f64>,
    exact: bool,
    anchor: bool,
    browser_native: bool,
}

#[derive(Serialize)]
struct OptionView<'a> {
    id: Uuid,
    source: &'a str,
    extension: &'a str,
    size_bytes: i64,
    exact: bool,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/playback/v1", post(start))
        .route("/api/playback/v1/{id}/switch", post(switch))
        .layer(DefaultBodyLimit::max(8 * 1024))
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::NOT_FOUND,
        "No compatible playback source is currently available.".into(),
    )
}

async fn choices(
    state: &AppState,
    viewer: Uuid,
    request: &Start,
    established: Option<(Uuid, Uuid)>,
) -> Result<Vec<Location>, AppError> {
    let key = catalog::file_key(&request.source_id, &request.relative_path);
    let remote = Uuid::parse_str(&request.relative_path).ok();
    sqlx::query_as(
        "WITH anchor AS (
            SELECT v.logical_id,l.id,l.representation_id,r.extension,r.content_sha256,e.canonical_id
            FROM catalog_matched_locations v JOIN catalog_locations l ON l.id=v.location_id
            JOIN catalog_representations r ON r.id=l.representation_id
            LEFT JOIN catalog_matching_current c ON c.location_id=l.id AND c.representation_id=r.id
            LEFT JOIN catalog_verified_identities e ON e.representation_id=r.id AND NOT COALESCE(c.suppress_verified,false)
            WHERE $6::uuid IS NULL AND v.viewer_id=$1 AND l.source_id=$2 AND
                (l.local_file_key=$3 OR (l.remote_representation_id IS NOT NULL AND r.id=$4))
            UNION ALL
            SELECT $7::uuid,l.id,r.id,r.extension,r.content_sha256,e.canonical_id
            FROM catalog_representations r
            LEFT JOIN catalog_locations l ON l.representation_id=r.id AND l.source_id=$2
            LEFT JOIN catalog_matching_current c ON c.location_id=l.id AND c.representation_id=r.id
            LEFT JOIN catalog_verified_identities e ON e.representation_id=r.id
                AND e.canonical_id=$7 AND NOT COALESCE(c.suppress_verified,false)
            WHERE r.id=$6
         )
         SELECT l.id,r.id AS representation_id,s.id AS source_id,s.name AS source_name,s.path AS root,
                l.local_relative_path AS path,s.node_id,r.extension,r.size_bytes,
                a.representation_id AS anchor_representation,a.logical_id AS logical,
                l.local_modified_at AS modified_at,
                (r.id=a.representation_id OR COALESCE(r.extension=a.extension AND
                    r.content_sha256=a.content_sha256,false)) AS exact,COALESCE(l.id=a.id,false) AS anchor,
                (s.node_id IS NULL OR i.kind='book' OR cardinality($5::text[])=0 OR r.extension=ANY($5)) AS browser_native
         FROM anchor a JOIN catalog_matched_locations v ON v.logical_id=a.logical_id AND v.viewer_id=$1
         JOIN catalog_locations l ON l.id=v.location_id JOIN sources s ON s.id=l.source_id
         JOIN catalog_representations r ON r.id=l.representation_id
         JOIN catalog_items i ON i.id=r.item_id
         LEFT JOIN catalog_matching_current c ON c.location_id=l.id AND c.representation_id=r.id
         LEFT JOIN catalog_verified_identities e ON e.representation_id=r.id AND NOT COALESCE(c.suppress_verified,false)
         WHERE v.playback_eligible AND (r.id=a.representation_id OR
               (r.extension=a.extension AND r.content_sha256=a.content_sha256) OR
               (i.kind='video' AND e.canonical_id=a.canonical_id))
           AND (s.kind='local_directory' OR EXISTS (
               SELECT 1 FROM remote_media_locations ml
               WHERE ml.source_id=s.id AND ml.node_id=s.node_id AND ml.representation_id=l.remote_representation_id))
           AND (i.kind<>'book' OR (r.extension='epub' AND r.size_bytes<=134217728)
                OR (r.extension='cbz' AND r.size_bytes<=4294967296))
           AND (s.kind='local_directory' OR i.kind='book' OR l.id=a.id OR r.extension=ANY($5))
         ORDER BY COALESCE(l.id=a.id,false) DESC, (s.node_id IS NULL) DESC, l.id LIMIT 32",
    )
    .bind(viewer).bind(&request.source_id).bind(key).bind(remote).bind(&request.native_formats)
    .bind(established.map(|(representation,_)|representation)).bind(established.map(|(_,logical)|logical))
    .fetch_all(&state.db).await.map_err(|_|auth::unavailable())
}

fn rank(state: &AppState, entry: &Location) -> (bool, u8, u8, u64, Uuid) {
    let health = entry
        .node_id
        .and_then(|node| state.relay.delivery_health(node));
    // Keep the requested encode when quality of another encode is unknown.
    let encode = u8::from(!entry.anchor && !entry.exact);
    let transport = match (entry.node_id, health) {
        (None, _) => 0,
        (Some(_), Some((_, false))) => 1,
        (Some(_), None) => 2,
        (Some(_), Some((_, true))) => 3,
    };
    (
        !entry.browser_native,
        encode,
        transport,
        health.map_or(u64::MAX, |(cost, _)| cost),
        entry.id,
    )
}

async fn materialize(
    state: &AppState,
    user: &auth::User,
    entry: &Location,
) -> Result<catalog::Item, AppError> {
    if let (Some(root), Some(path)) = (&entry.root, &entry.path) {
        let kind = crate::classify_extension(&entry.extension).ok_or_else(unavailable)?;
        let root = std::path::PathBuf::from(root);
        let relative = path.clone();
        let permit = LOCAL_PROBES.try_acquire().map_err(|_| unavailable())?;
        let metadata = tokio::time::timeout(
            Duration::from_secs(2),
            tokio::task::spawn_blocking(move || {
                let _permit = permit;
                crate::resolve_media_path(&root, &relative, kind)?
                    .metadata()
                    .map_err(|_| unavailable())
            }),
        )
        .await
        .map_err(|_| unavailable())?
        .map_err(|_| unavailable())??;
        let modified = crate::modified_epoch(&metadata);
        if metadata.len() != entry.size_bytes as u64
            || entry
                .modified_at
                .is_some_and(|stamp| Some(stamp) != modified)
        {
            return Err(unavailable());
        }
        crate::media_root(state, user, &entry.source_id).await?;
        let mut item = catalog::item(
            &entry.source_id,
            &entry.source_name,
            MediaFile {
                name: std::path::Path::new(path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                relative_path: path.clone(),
                kind,
                extension: entry.extension.clone(),
                size_bytes: metadata.len(),
                modified_at: modified,
            },
        );
        catalog::enrich(state, user.id, std::slice::from_mut(&mut item)).await?;
        unified_catalog::annotate_local(state, user.id, std::slice::from_mut(&mut item)).await?;
        return Ok(item);
    }
    let (items, _) =
        unified_catalog::remote_items(state, user.id, Some(&[entry.representation_id]), None)
            .await?;
    items
        .into_iter()
        .find(|item| item.available)
        .ok_or_else(unavailable)
}

fn reply(
    id: Uuid,
    selected: &Location,
    item: catalog::Item,
    options: &[Location],
    exact: bool,
) -> Response {
    let options: Vec<_> = options
        .iter()
        .take(MAX_OPTIONS)
        .map(|entry| OptionView {
            id: entry.id,
            source: &entry.source_name,
            extension: &entry.extension,
            size_bytes: entry.size_bytes,
            exact: entry.exact,
        })
        .collect();
    (
        [("cache-control", "private, no-store")],
        Json(serde_json::json!({
            "session":id,"location":selected.id,"item":item,"options":options,"exact":exact
        })),
    )
        .into_response()
}

async fn start(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(request): Json<Start>,
) -> Result<Response, AppError> {
    if request.source_id.len() > 128
        || request.relative_path.len() > 4096
        || request.native_formats.len() > 24
        || request
            .native_formats
            .iter()
            .any(|format| format.len() > 12 || !format.bytes().all(|c| c.is_ascii_alphanumeric()))
    {
        return Err(crate::bad_request("Invalid playback reference."));
    }
    auth::rate_limit(&state.db, format!("playback:{}", identity.user.id), 90, 60).await?;
    let mut options = choices(&state, identity.user.id, &request, None).await?;
    options.sort_by_key(|entry| rank(&state, entry));
    for entry in options.iter().take(MAX_OPTIONS) {
        if let Ok(item) = materialize(&state, &identity.user, entry).await {
            let id = Uuid::new_v4();
            let now = Instant::now();
            let mut sessions = state.playback.0.lock().unwrap();
            sessions.retain(|_, session| session.expires > now);
            let own = sessions
                .iter()
                .filter(|(_, session)| session.owner == identity.user.id);
            if own.clone().count() >= 24 {
                if let Some(oldest) = own
                    .min_by_key(|(_, session)| session.expires)
                    .map(|(id, _)| *id)
                {
                    sessions.remove(&oldest);
                }
            }
            if sessions.len() >= MAX_SESSIONS {
                return Err(AppError(
                    StatusCode::TOO_MANY_REQUESTS,
                    "Playback selection is busy.".into(),
                ));
            }
            sessions.insert(
                id,
                Session {
                    auth_session: identity.session_hash,
                    owner: identity.user.id,
                    anchor_representation: entry.anchor_representation,
                    logical: entry.logical,
                    request,
                    location: entry.id,
                    failed: Vec::new(),
                    exact: entry.exact,
                    expires: now + TTL,
                },
            );
            return Ok(reply(id, entry, item, &options, entry.exact));
        }
    }
    Err(unavailable())
}

async fn switch(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    Json(input): Json<Switch>,
) -> Result<Response, AppError> {
    auth::rate_limit(
        &state.db,
        format!("playback:switch:{}", identity.user.id),
        30,
        60,
    )
    .await?;
    let mut session = state
        .playback
        .0
        .lock()
        .unwrap()
        .get(&id)
        .cloned()
        .filter(|s| {
            s.expires > Instant::now()
                && s.auth_session == identity.session_hash
                && s.location == input.current
        })
        .ok_or_else(unavailable)?;
    let mut options = choices(
        &state,
        identity.user.id,
        &session.request,
        Some((session.anchor_representation, session.logical)),
    )
    .await?;
    options.sort_by_key(|entry| rank(&state, entry));
    if input.prefer.is_none() {
        if session.failed.len() >= MAX_FAILOVERS {
            return Err(unavailable());
        }
        if !session.failed.contains(&session.location) {
            session.failed.push(session.location);
        }
        options.retain(|entry| !session.failed.contains(&entry.id));
    }
    for entry in options
        .iter()
        .take(MAX_OPTIONS)
        .filter(|entry| input.prefer.is_none_or(|id| entry.id == id))
    {
        if let Ok(item) = materialize(&state, &identity.user, entry).await {
            let mut sessions = state.playback.0.lock().unwrap();
            let current = sessions
                .get_mut(&id)
                .filter(|s| s.location == input.current)
                .ok_or_else(unavailable)?;
            let exact = entry.id == input.current || (entry.exact && session.exact);
            session.exact = entry.exact;
            session.location = entry.id;
            *current = session;
            return Ok(reply(id, entry, item, &options, exact));
        }
    }
    if input.prefer.is_none() {
        if let Some(current) = state
            .playback
            .0
            .lock()
            .unwrap()
            .get_mut(&id)
            .filter(|s| s.location == input.current)
        {
            current.failed = session.failed;
        }
    }
    Err(unavailable())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_integration::{body, register_user, request, state};
    use axum::http::Method;
    use serde_json::json;
    use sqlx::PgPool;

    #[sqlx::test(migrations = "./migrations")]
    #[ignore]
    async fn routing_revalidates_switches_and_rejects_unrelated_or_revoked_locations(pool: PgPool) {
        let state = state(pool.clone());
        let (cookie, csrf, user) = register_user(&state, "routing_viewer").await;
        let (other_cookie, other_csrf, other) = register_user(&state, "routing_owner").await;
        let viewer: Uuid = serde_json::from_value(user["id"].clone()).unwrap();
        let owner: Uuid = serde_json::from_value(other["id"].clone()).unwrap();
        let root = std::env::temp_dir().join(format!("grimoire-route-{}", Uuid::new_v4()));
        std::fs::create_dir(&root).unwrap();
        struct Fixture(std::path::PathBuf);
        impl Drop for Fixture {
            fn drop(&mut self) {
                let _ = std::fs::remove_dir_all(&self.0);
            }
        }
        let _fixture = Fixture(root.clone());
        let mut observations = Vec::new();
        for (source, name, bytes) in [
            ("preferred", "film.mp4", b"source A".as_slice()),
            ("alternate", "film.mkv", b"source B encode".as_slice()),
            ("forbidden", "film.mp4", b"private".as_slice()),
            ("different", "film.mp4", b"different cut".as_slice()),
        ] {
            let path = root.join(source);
            std::fs::create_dir(&path).unwrap();
            std::fs::write(path.join(name), bytes).unwrap();
            sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,$1,$3)")
                .bind(source)
                .bind(owner)
                .bind(path.to_str().unwrap())
                .execute(&pool)
                .await
                .unwrap();
            if source != "forbidden" {
                sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES($1,$2,true,true)")
                    .bind(source).bind(viewer).execute(&pool).await.unwrap();
            }
            observations.push(json!({"file_key":catalog::file_key(source,name),"source_id":source,
                "path":name,"kind":"video","extension":name.split('.').next_back().unwrap(),"size":bytes.len()}));
        }
        crate::catalog_inventory::record(&pool, json!(observations))
            .await
            .unwrap();
        let canonical:Uuid=sqlx::query_scalar("SELECT r.item_id FROM catalog_representations r JOIN catalog_locations l ON l.representation_id=r.id WHERE l.source_id='preferred'").fetch_one(&pool).await.unwrap();
        sqlx::query("INSERT INTO catalog_verified_identities(representation_id,namespace,entity_type,identifier,edition,canonical_id)
            SELECT representation_id,'tmdb_movie','movie','42','theatrical',$1 FROM catalog_locations
            WHERE source_id IN ('preferred','alternate','forbidden')")
            .bind(canonical).execute(&pool).await.unwrap();
        let post = |url, data| {
            request(
                &state,
                Method::POST,
                url,
                Some(&cookie),
                Some(&csrf),
                Some("http://localhost:5173"),
                Some(data),
            )
        };
        let start = body(
            post(
                "/api/playback/v1",
                json!({"source_id":"preferred","relative_path":"film.mp4"}),
            )
            .await,
        )
        .await;
        assert_eq!(start["item"]["source_id"], "preferred");
        let options = start["options"].as_array().unwrap();
        assert_eq!(options.len(), 2);
        assert!(!start.to_string().contains("forbidden"));
        assert!(!start.to_string().contains("different"));
        assert!(!start.to_string().contains(root.to_str().unwrap()));
        let alternate = options
            .iter()
            .find(|option| option["source"] == "alternate")
            .unwrap()["id"]
            .clone();
        let route = format!(
            "/api/playback/v1/{}/switch",
            start["session"].as_str().unwrap()
        );
        let input = json!({"current":start["location"],"prefer":alternate});
        assert_eq!(
            request(
                &state,
                Method::POST,
                &route,
                Some(&other_cookie),
                Some(&other_csrf),
                Some("http://localhost:5173"),
                Some(input.clone())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        let manual = body(post(&route, input).await).await;
        assert_eq!(manual["item"]["source_id"], "alternate");
        assert_eq!(manual["exact"], false);
        assert_eq!(
            post(&route, json!({"current":start["location"]}))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
        sqlx::query("DELETE FROM source_permissions WHERE source_id='preferred'")
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            post(
                &route,
                json!({"current":manual["location"],"prefer":start["location"]})
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES('preferred',$1,true,true)")
            .bind(viewer).execute(&pool).await.unwrap();
        let fresh = body(
            post(
                "/api/playback/v1",
                json!({"source_id":"preferred","relative_path":"film.mp4"}),
            )
            .await,
        )
        .await;
        let fresh_route = format!(
            "/api/playback/v1/{}/switch",
            fresh["session"].as_str().unwrap()
        );
        sqlx::query("DELETE FROM source_permissions WHERE source_id='preferred'")
            .execute(&pool)
            .await
            .unwrap();
        let failover = body(post(&fresh_route, json!({"current":fresh["location"]})).await).await;
        assert_eq!(failover["item"]["source_id"], "alternate");
        assert_eq!(failover["options"].as_array().unwrap().len(), 1);
        assert_eq!(
            post(&fresh_route, json!({"current":failover["location"]}))
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
}
