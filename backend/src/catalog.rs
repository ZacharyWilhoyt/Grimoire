//! Live, bounded catalog scans plus per-account saved items and open history.
use crate::{auth, media_root, resolve_media_path, scan_directory, AppError, AppState, MediaFile};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
};
use uuid::Uuid;

const MAX_SOURCES: usize = 32;
const MAX_ITEMS: usize = 5_000;
const PAGE_SIZE: i64 = 100;
#[derive(FromRow)]
struct MediaSource {
    id: String,
    name: String,
    path: String,
}
#[derive(Clone, Serialize)]
pub struct Item {
    pub progress: Option<crate::progress::Summary>,
    pub file_key: String,
    pub source_id: String,
    pub source_name: String,
    #[serde(flatten)]
    pub file: MediaFile,
    pub saved_at: Option<f64>,
    pub last_opened_at: Option<f64>,
    pub available: bool,
    pub metadata: Option<crate::metadata::View>,
}
#[derive(Serialize, FromRow)]
pub struct PersonalState {
    pub file_key: String,
    pub saved_at: Option<f64>,
    pub last_opened_at: Option<f64>,
}
#[derive(Serialize)]
pub struct Catalog {
    items: Vec<Item>,
    truncated: bool,
    unavailable_sources: usize,
}
#[derive(Serialize)]
pub struct PersonalPage {
    items: Vec<Item>,
    has_more: bool,
    next_offset: i64,
    unavailable_items: usize,
}
#[derive(Deserialize, Default)]
pub struct Page {
    #[serde(default)]
    offset: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MediaRef {
    source_id: String,
    relative_path: String,
}
#[derive(FromRow)]
struct PersonalRow {
    file_key: String,
    source_id: String,
    source_name: String,
    path: String,
    relative_path: String,
    saved_at: Option<f64>,
    last_opened_at: Option<f64>,
}

pub fn file_key(source: &str, path: &str) -> String {
    auth::digest(&format!("{source}\0{path}"))
}
fn item(source: &MediaSource, file: MediaFile) -> Item {
    Item {
        file_key: file_key(&source.id, &file.relative_path),
        source_id: source.id.clone(),
        source_name: source.name.clone(),
        file,
        saved_at: None,
        last_opened_at: None,
        available: true,
        metadata: None,
        progress: None,
    }
}
async fn enrich(state: &AppState, user: Uuid, items: &mut [Item]) -> Result<(), AppError> {
    let keys: Vec<_> = items.iter().map(|item| item.file_key.clone()).collect();
    let rows = sqlx::query_as::<_, PersonalState>("SELECT file_key,extract(epoch from saved_at)::float8 AS saved_at,extract(epoch from last_opened_at)::float8 AS last_opened_at FROM user_media WHERE user_id=$1 AND file_key=ANY($2)")
        .bind(user).bind(keys).fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    let by_key: HashMap<_, _> = rows
        .into_iter()
        .map(|row| (row.file_key.clone(), row))
        .collect();
    for item in items.iter_mut() {
        if let Some(row) = by_key.get(&item.file_key) {
            item.saved_at = row.saved_at;
            item.last_opened_at = row.last_opened_at;
        }
    }
    crate::metadata::enrich(state, user, items).await?;
    crate::progress::enrich(state, user, items).await?;
    Ok(())
}
// Recheck after filesystem work so revocation during a long scan cannot expose metadata.
async fn filter_current_access(
    state: &AppState,
    user: Uuid,
    items: &mut Vec<Item>,
) -> Result<(), AppError> {
    let ids: Vec<_> = items
        .iter()
        .map(|item| item.source_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let allowed: Vec<String> = sqlx::query_scalar("SELECT s.id FROM sources s WHERE s.id=ANY($2) AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media))")
        .bind(user).bind(ids).fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    let allowed: HashSet<_> = allowed.into_iter().collect();
    items.retain(|item| allowed.contains(&item.source_id));
    Ok(())
}
async fn scan_permit(state: &AppState) -> Result<tokio::sync::OwnedSemaphorePermit, AppError> {
    state
        .catalog_scans
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::TOO_MANY_REQUESTS,
                "The catalog is busy. Please refresh in a moment.".into(),
            )
        })
}

pub async fn discover(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<Json<Catalog>, AppError> {
    let sources = sqlx::query_as::<_, MediaSource>("SELECT s.id,s.name,s.path FROM sources s WHERE s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media) ORDER BY s.created_at,s.id LIMIT 33")
        .bind(identity.user.id).fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    let permit = scan_permit(&state).await?;
    let mut result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut result = Catalog {
            items: Vec::new(),
            truncated: sources.len() > MAX_SOURCES,
            unavailable_sources: 0,
        };
        for source in sources.into_iter().take(MAX_SOURCES) {
            if std::fs::read_dir(&source.path).is_err() {
                result.unavailable_sources += 1;
                continue;
            }
            let files = scan_directory(std::path::Path::new(&source.path)).unwrap_or_default();
            if files.len() >= crate::MAX_MEDIA_FILES {
                result.truncated = true;
            }
            for file in files {
                if result.items.len() >= MAX_ITEMS {
                    result.truncated = true;
                    break;
                }
                result.items.push(item(&source, file));
            }
            if result.items.len() >= MAX_ITEMS {
                result.truncated = true;
                break;
            }
        }
        result
    })
    .await
    .map_err(|_| auth::unavailable())?;
    filter_current_access(&state, identity.user.id, &mut result.items).await?;
    enrich(&state, identity.user.id, &mut result.items).await?;
    Ok(Json(result))
}

pub async fn source_items(
    state: &AppState,
    user: &auth::User,
    source_id: &str,
) -> Result<Vec<Item>, AppError> {
    let root = media_root(state, user, source_id).await?;
    let name: String = sqlx::query_scalar("SELECT name FROM sources WHERE id=$1")
        .bind(source_id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    if std::fs::read_dir(&root).is_err() {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "This source is currently unavailable.".into(),
        ));
    }
    let permit = scan_permit(state).await?;
    let files = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        scan_directory(&root)
    })
    .await
    .map_err(|_| auth::unavailable())?
    .map_err(|_| auth::unavailable())?;
    let source = MediaSource {
        id: source_id.into(),
        name,
        path: String::new(),
    };
    let mut items: Vec<_> = files.into_iter().map(|file| item(&source, file)).collect();
    media_root(state, user, source_id).await?;
    enrich(state, user.id, &mut items).await?;
    Ok(items)
}

async fn personal_page(
    state: AppState,
    user: Uuid,
    offset: i64,
    mode: u8,
) -> Result<Json<PersonalPage>, AppError> {
    if !(0..=1_000_000).contains(&offset) {
        return Err(crate::bad_request("Invalid page offset."));
    }
    let (column, order) = if mode == 1 {
        ("last_opened_at", "m.last_opened_at")
    } else {
        ("saved_at", "m.saved_at")
    };
    let sql = format!("SELECT m.file_key,m.source_id,s.name AS source_name,s.path,m.relative_path,extract(epoch from m.saved_at)::float8 AS saved_at,extract(epoch from m.last_opened_at)::float8 AS last_opened_at FROM user_media m JOIN sources s ON s.id=m.source_id WHERE m.user_id=$1 AND m.{column} IS NOT NULL AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media)) ORDER BY {order} DESC,m.file_key LIMIT 101 OFFSET $2");
    let sql = if mode == 2 {
        "SELECT m.file_key,m.source_id,s.name AS source_name,s.path,m.relative_path,extract(epoch from u.saved_at)::float8 AS saved_at,extract(epoch from u.last_opened_at)::float8 AS last_opened_at FROM media_progress m JOIN sources s ON s.id=m.source_id LEFT JOIN user_media u ON u.user_id=m.user_id AND u.file_key=m.file_key WHERE m.user_id=$1 AND m.position>0 AND (m.total IS NULL OR m.position<m.total) AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media)) ORDER BY m.updated_at DESC,m.file_key LIMIT 101 OFFSET $2".to_string()
    } else {
        sql
    };
    let rows = sqlx::query_as::<_, PersonalRow>(&sql)
        .bind(user)
        .bind(offset)
        .fetch_all(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    let has_more = rows.len() > PAGE_SIZE as usize;
    let permit = scan_permit(&state).await?;
    let mut result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut items = Vec::new();
        let mut unavailable_items = 0;
        for row in rows.into_iter().take(PAGE_SIZE as usize) {
            let extension = std::path::Path::new(&row.relative_path)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let Some(kind) = crate::classify_extension(&extension) else {
                unavailable_items += 1;
                continue;
            };
            let metadata =
                resolve_media_path(std::path::Path::new(&row.path), &row.relative_path, kind)
                    .ok()
                    .and_then(|path| path.metadata().ok());
            let available = metadata.is_some();
            if !available {
                unavailable_items += 1;
            }
            let file = MediaFile {
                name: std::path::Path::new(&row.relative_path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into(),
                relative_path: row.relative_path,
                kind,
                extension,
                size_bytes: metadata.as_ref().map_or(0, |metadata| metadata.len()),
                modified_at: metadata.as_ref().and_then(crate::modified_epoch),
            };
            items.push(Item {
                file_key: row.file_key,
                source_id: row.source_id,
                source_name: row.source_name,
                file,
                saved_at: row.saved_at,
                last_opened_at: row.last_opened_at,
                available,
                metadata: None,
                progress: None,
            });
        }
        PersonalPage {
            items,
            has_more,
            next_offset: offset + PAGE_SIZE,
            unavailable_items,
        }
    })
    .await
    .map_err(|_| auth::unavailable())?;
    filter_current_access(&state, user, &mut result.items).await?;
    crate::metadata::enrich(&state, user, &mut result.items).await?;
    crate::progress::enrich(&state, user, &mut result.items).await?;
    Ok(Json(result))
}
pub async fn library(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(page): Query<Page>,
) -> Result<Json<PersonalPage>, AppError> {
    personal_page(state, identity.user.id, page.offset, 0).await
}
pub async fn recent(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(page): Query<Page>,
) -> Result<Json<PersonalPage>, AppError> {
    personal_page(state, identity.user.id, page.offset, 1).await
}
async fn set_state(
    state: AppState,
    user: Uuid,
    reference: MediaRef,
    save: bool,
) -> Result<Json<PersonalState>, AppError> {
    if reference.relative_path.len() > 4096 || reference.source_id.len() > 128 {
        return Err(crate::bad_request("Invalid media reference."));
    }
    let root = media_root(
        &state,
        &auth::User {
            id: user,
            username: String::new(),
            is_admin: false,
        },
        &reference.source_id,
    )
    .await?;
    let path = PathBuf::from(&reference.relative_path);
    let extension = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind = crate::classify_extension(&extension)
        .ok_or_else(|| crate::bad_request("Unsupported media file."))?;
    let relative = reference.relative_path.clone();
    tokio::task::spawn_blocking(move || {
        let canonical = resolve_media_path(&root, &relative, kind)?;
        if canonical
            .strip_prefix(&root)
            .ok()
            .and_then(|path| path.to_str())
            != Some(relative.as_str())
        {
            return Err(crate::bad_request(
                "Use the media path returned by the catalog.",
            ));
        }
        Ok(())
    })
    .await
    .map_err(|_| auth::unavailable())??;
    let column = if save { "saved_at" } else { "last_opened_at" };
    let update_value = if save {
        "COALESCE(user_media.saved_at,EXCLUDED.saved_at)"
    } else {
        "EXCLUDED.last_opened_at"
    };
    let sql = format!("INSERT INTO user_media(user_id,file_key,source_id,relative_path,{column}) SELECT $1,$2,$3,$4,now() FROM sources s WHERE s.id=$3 AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media)) ON CONFLICT(user_id,file_key) DO UPDATE SET {column}={update_value} RETURNING file_key,extract(epoch from saved_at)::float8 AS saved_at,extract(epoch from last_opened_at)::float8 AS last_opened_at");
    let row = sqlx::query_as::<_, PersonalState>(&sql)
        .bind(user)
        .bind(file_key(&reference.source_id, &reference.relative_path))
        .bind(reference.source_id)
        .bind(reference.relative_path)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| auth::unavailable())?
        .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Media access not granted.".into()))?;
    Ok(Json(row))
}
pub async fn save(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(reference): Json<MediaRef>,
) -> Result<Json<PersonalState>, AppError> {
    set_state(state, identity.user.id, reference, true).await
}
pub async fn opened(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(reference): Json<MediaRef>,
) -> Result<Json<PersonalState>, AppError> {
    set_state(state, identity.user.id, reference, false).await
}
pub async fn unsave(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(key): Path<String>,
) -> Result<StatusCode, AppError> {
    if key.len() != 64 || !key.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(crate::bad_request("Invalid media key."));
    }
    sqlx::query("UPDATE user_media SET saved_at=NULL WHERE user_id=$1 AND file_key=$2")
        .bind(identity.user.id)
        .bind(key)
        .execute(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn continue_media(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(page): Query<Page>,
) -> Result<Json<PersonalPage>, AppError> {
    personal_page(state, identity.user.id, page.offset, 2).await
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LinkQuery {
    source_id: String,
    path: Option<String>,
    group: Option<String>,
}
#[derive(Serialize)]
pub struct LinkContext {
    item: Option<Item>,
    members: Vec<Item>,
    label: Option<String>,
}
// URLs identify resources, never grant access. Resolve the logged-in viewer's live catalog.
pub async fn linked(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    Query(query): Query<LinkQuery>,
) -> Result<Json<LinkContext>, AppError> {
    if query.source_id.len() > 128
        || query.path.as_ref().is_some_and(|v| v.len() > 4096)
        || query.group.as_ref().is_some_and(|v| v.len() != 64)
        || query.path.is_some() == query.group.is_some()
    {
        return Err(crate::bad_request("Invalid media link."));
    }
    let mut items = source_items(&state, &identity.user, &query.source_id).await?;
    let selected = query.path.as_ref().and_then(|path| {
        items
            .iter()
            .position(|item| item.file.relative_path == *path)
    });
    if query.path.is_some() && selected.is_none() {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "Media is unavailable or access has not been granted.".into(),
        ));
    }
    let item = selected.map(|index| items.remove(index));
    let group = query.group.or_else(|| {
        item.as_ref()?
            .metadata
            .as_ref()?
            .group
            .as_ref()
            .map(|group| group.id.clone())
    });
    items.retain(|item| {
        group.as_ref().is_some_and(|group| {
            item.metadata
                .as_ref()
                .and_then(|m| m.group.as_ref())
                .is_some_and(|g| &g.id == group)
        })
    });
    if let Some(ref item) = item {
        items.push(item.clone());
    }
    if items.is_empty() {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "Series is unavailable or access has not been granted.".into(),
        ));
    }
    items.sort_by(|a, b| {
        let order = |item: &Item| {
            let fields = item.metadata.as_ref().map(|m| &m.fields);
            (
                fields.and_then(|f| f.season).unwrap_or(0),
                fields.and_then(|f| f.episode).unwrap_or(0),
            )
        };
        order(a)
            .cmp(&order(b))
            .then_with(|| natord::compare(&a.file.relative_path, &b.file.relative_path))
    });
    let label = items
        .first()
        .and_then(|i| i.metadata.as_ref()?.group.as_ref().map(|g| g.label.clone()));
    media_root(&state, &identity.user, &query.source_id).await?;
    Ok(Json(LinkContext {
        item,
        members: items,
        label,
    }))
}
