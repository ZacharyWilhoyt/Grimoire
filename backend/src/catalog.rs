//! Live, bounded catalog scans plus per-account saved items and open history.
use crate::{
    auth, media_root, resolve_media_path, scan_directory, sources::LocalSource, unified_catalog,
    AppError, AppState, MediaFile,
};
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
    #[serde(skip_serializing_if = "Option::is_none")]
    pub logical_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub location_id: Option<Uuid>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collection_link: Option<crate::collection_membership::Collection>,
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub remote: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub media_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remote_access: Option<bool>,
}
#[derive(Serialize, FromRow)]
pub struct PersonalState {
    pub file_key: String,
    pub saved_at: Option<f64>,
    pub last_opened_at: Option<f64>,
}
#[derive(Serialize)]
pub struct Catalog {
    pub(crate) items: Vec<Item>,
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
    path: Option<String>,
    remote_representation: Option<Uuid>,
    relative_path: String,
    saved_at: Option<f64>,
    last_opened_at: Option<f64>,
}

pub fn file_key(source: &str, path: &str) -> String {
    auth::digest(&format!("{source}\0{path}"))
}
pub(crate) fn item(source_id: &str, source_name: &str, file: MediaFile) -> Item {
    Item {
        file_key: file_key(source_id, &file.relative_path),
        source_id: source_id.into(),
        source_name: source_name.into(),
        file,
        saved_at: None,
        last_opened_at: None,
        available: true,
        metadata: None,
        progress: None,
        logical_id: None,
        location_id: None,
        collection_link: None,
        remote: false,
        media_url: None,
        remote_access: None,
    }
}
pub(crate) async fn enrich_personal(
    state: &AppState,
    user: Uuid,
    items: &mut [Item],
) -> Result<(), AppError> {
    if items.is_empty() {
        return Ok(());
    }
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
    crate::progress::enrich(state, user, items).await?;
    Ok(())
}
pub(crate) async fn enrich(
    state: &AppState,
    user: Uuid,
    items: &mut [Item],
) -> Result<(), AppError> {
    enrich_personal(state, user, items).await?;
    crate::metadata::enrich(state, user, items).await?;
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
    let sources = sqlx::query_as::<_, MediaSource>("SELECT s.id,s.name,s.path FROM sources s WHERE s.kind='local_directory' AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media)) ORDER BY s.created_at,s.id LIMIT 33")
        .bind(identity.user.id).fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    let permit = if sources.is_empty() {
        None
    } else {
        Some(scan_permit(&state).await?)
    };
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
                result.items.push(item(&source.id, &source.name, file));
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
    crate::catalog_inventory::observe(&state, &result.items);
    enrich(&state, identity.user.id, &mut result.items).await?;
    unified_catalog::annotate_local(&state, identity.user.id, &mut result.items).await?;
    let (mut remote, truncated) =
        unified_catalog::remote_items(&state, identity.user.id, None, None).await?;
    enrich_personal(&state, identity.user.id, &mut remote).await?;
    result.items.extend(remote);
    crate::collection_membership::annotate(&state, identity.user.id, &mut result.items).await?;
    result.truncated |= truncated;
    Ok(Json(result))
}

pub async fn source_items(
    state: &AppState,
    user: &auth::User,
    source_id: &str,
) -> Result<Vec<Item>, AppError> {
    let source = LocalSource::authorized(state, user.id, source_id).await?;
    let permit = scan_permit(state).await?;
    let mut items = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        Ok::<_, AppError>(
            source
                .list()?
                .into_iter()
                .map(|file| item(&source.id, &source.name, file))
                .collect::<Vec<_>>(),
        )
    })
    .await
    .map_err(|_| auth::unavailable())??;
    media_root(state, user, source_id).await?;
    crate::catalog_inventory::observe(state, &items);
    enrich(state, user.id, &mut items).await?;
    unified_catalog::annotate_local(state, user.id, &mut items).await?;
    crate::collection_membership::annotate(state, user.id, &mut items).await?;
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
    let joins = "JOIN sources s ON s.id=m.source_id
        LEFT JOIN catalog_remote_page_locations rv ON s.kind='remote_agent'
             AND rv.source_id=s.id AND rv.viewer_id=$1 AND rv.representation_id::text=m.relative_path";
    let access = "(s.kind='local_directory' AND (s.owner_id=$1 OR EXISTS(
        SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1
          AND p.can_discover AND p.can_access_media))
        OR s.kind='remote_agent' AND rv.representation_id IS NOT NULL
          AND (rv.media_granted OR $3=0))";
    let fields = "m.file_key,m.source_id,s.name AS source_name,s.path,
                  rv.representation_id AS remote_representation,m.relative_path";
    let sql = if mode == 2 {
        format!("SELECT {fields},extract(epoch from u.saved_at)::float8 AS saved_at,
                 extract(epoch from u.last_opened_at)::float8 AS last_opened_at
                 FROM media_progress m {joins}
                 LEFT JOIN user_media u ON u.user_id=m.user_id AND u.file_key=m.file_key
                 WHERE m.user_id=$1 AND {access}
                 AND (NOT m.completed OR COALESCE(rv.extension NOT IN ('epub','cbz'),
                      lower(m.relative_path) NOT LIKE '%.epub' AND lower(m.relative_path) NOT LIKE '%.cbz'))
                 AND (m.position>0 OR m.locator IS NOT NULL) AND (m.total IS NULL OR m.position<m.total)
                 ORDER BY m.updated_at DESC,m.file_key LIMIT 101 OFFSET $2")
    } else {
        format!("SELECT {fields},extract(epoch from m.saved_at)::float8 AS saved_at,
                 extract(epoch from m.last_opened_at)::float8 AS last_opened_at
                 FROM user_media m {joins} WHERE m.user_id=$1 AND m.{column} IS NOT NULL AND {access}
                 ORDER BY {order} DESC,m.file_key LIMIT 101 OFFSET $2")
    };
    let rows = sqlx::query_as::<_, PersonalRow>(&sql)
        .bind(user)
        .bind(offset)
        .bind(i32::from(mode))
        .fetch_all(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    let has_more = rows.len() > PAGE_SIZE as usize;
    let ordering: HashMap<_, _> = rows
        .iter()
        .enumerate()
        .map(|(index, row)| (row.file_key.clone(), index))
        .collect();
    let (remote_rows, local_rows): (Vec<_>, Vec<_>) = rows
        .into_iter()
        .take(PAGE_SIZE as usize)
        .partition(|row| row.remote_representation.is_some());
    let permit = if local_rows.is_empty() {
        None
    } else {
        Some(scan_permit(&state).await?)
    };
    let mut result = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let mut items = Vec::new();
        let mut unavailable_items = 0;
        for row in local_rows {
            let extension = std::path::Path::new(&row.relative_path)
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let Some(kind) = crate::classify_extension(&extension) else {
                unavailable_items += 1;
                continue;
            };
            let metadata = row
                .path
                .as_deref()
                .and_then(|path| {
                    resolve_media_path(std::path::Path::new(path), &row.relative_path, kind).ok()
                })
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
                logical_id: None,
                location_id: None,
                collection_link: None,
                remote: false,
                media_url: None,
                remote_access: None,
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
    unified_catalog::annotate_local(&state, user, &mut result.items).await?;
    let remote_states: HashMap<_, _> = remote_rows
        .into_iter()
        .filter_map(|row| {
            Some((
                row.remote_representation?,
                (row.saved_at, row.last_opened_at),
            ))
        })
        .collect();
    if !remote_states.is_empty() {
        let ids: Vec<_> = remote_states.keys().copied().collect();
        let (mut remote, _) = unified_catalog::remote_items(&state, user, Some(&ids), None).await?;
        if mode != 0 {
            remote.retain(|item| item.remote_access == Some(true));
        }
        for item in &mut remote {
            if let Some(&(saved, opened)) = Uuid::parse_str(&item.file.relative_path)
                .ok()
                .and_then(|id| remote_states.get(&id))
            {
                item.saved_at = saved;
                item.last_opened_at = opened;
            }
        }
        result.unavailable_items += remote.iter().filter(|item| !item.available).count();
        result.items.extend(remote);
        result
            .items
            .sort_by_key(|item| ordering.get(&item.file_key).copied().unwrap_or(usize::MAX));
    }
    crate::progress::enrich(&state, user, &mut result.items).await?;
    crate::collection_membership::annotate(&state, user, &mut result.items).await?;
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
    if reference
        .source_id
        .starts_with(unified_catalog::REMOTE_PREFIX)
    {
        let id =
            Uuid::parse_str(&reference.relative_path).map_err(|_| unified_catalog::missing())?;
        if id.to_string() != reference.relative_path {
            return Err(unified_catalog::missing());
        }
        let (column, update) = if save {
            (
                "saved_at",
                "COALESCE(user_media.saved_at,EXCLUDED.saved_at)",
            )
        } else {
            ("last_opened_at", "EXCLUDED.last_opened_at")
        };
        let sql = format!(
            "INSERT INTO user_media(user_id,file_key,source_id,relative_path,{column})
            SELECT $1,$2,v.source_id,$3::uuid::text,now() FROM catalog_remote_page_locations v
            WHERE v.viewer_id=$1 AND v.source_id=$4 AND v.representation_id=$3
               AND ($5 OR v.playback_eligible)
            ON CONFLICT(user_id,file_key) DO UPDATE SET {column}={update}
            RETURNING file_key,extract(epoch from saved_at)::float8 AS saved_at,
            extract(epoch from last_opened_at)::float8 AS last_opened_at"
        );
        let row = sqlx::query_as::<_, PersonalState>(&sql)
            .bind(user)
            .bind(file_key(&reference.source_id, &reference.relative_path))
            .bind(id)
            .bind(&reference.source_id)
            .bind(save)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| auth::unavailable())?
            .ok_or_else(unified_catalog::missing)?;
        return Ok(Json(row));
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
    season: Option<String>,
}
#[derive(Serialize)]
pub struct LinkContext {
    pub(crate) item: Option<Item>,
    pub(crate) members: Vec<Item>,
    pub(crate) label: Option<String>,
    pub(crate) artwork: crate::metadata::sidecars::SeriesArtwork,
    pub(crate) seasons: Vec<crate::metadata::sidecars::Season>,
    pub(crate) season: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) series_source_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) series_group_id: Option<String>,
}
// URLs identify resources, never grant access. Resolve the logged-in viewer's live catalog.
pub async fn linked(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(query): Query<LinkQuery>,
) -> Result<Json<LinkContext>, AppError> {
    let source = query.source_id.clone();
    let mut context = linked_native(
        State(state.clone()),
        Extension(identity.clone()),
        Query(query),
    )
    .await?
    .0;
    crate::collection_membership::extend(&state, &identity, &source, &mut context).await?;
    if context.members.is_empty() {
        return Err(unified_catalog::missing());
    }
    Ok(Json(context))
}

async fn linked_native(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<auth::Identity>,
    Query(query): Query<LinkQuery>,
) -> Result<Json<LinkContext>, AppError> {
    use crate::metadata::sidecars;
    if query.season.as_ref().is_some_and(|key| {
        query.group.is_none()
            || (key != "unknown"
                && key
                    .parse::<u32>()
                    .map_or(true, |n| n > 100_000 || n.to_string() != *key))
    }) || query.source_id.len() > 128
        || query.path.as_ref().is_some_and(|v| v.len() > 4096)
        || query.group.as_ref().is_some_and(|v| v.len() != 64)
        || query.path.is_some() == query.group.is_some()
    {
        return Err(crate::bad_request("Invalid media link."));
    }
    if query.source_id.starts_with(unified_catalog::REMOTE_PREFIX) {
        return unified_catalog::remote_link(
            &state,
            identity.user.id,
            query.source_id,
            query.path,
            query.group,
            query.season,
        )
        .await
        .map(Json);
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
    unified_catalog::annotate_local(&state, identity.user.id, &mut items).await?;
    let mut item = selected.map(|index| items.remove(index));
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
    let reviewed: HashSet<_> = items
        .iter()
        .filter_map(|item| {
            let metadata = item.metadata.as_ref()?;
            let id = metadata.provider_id.as_deref()?;
            (group.is_some()
                && metadata.provider.as_deref() == Some("tmdb_tv")
                && metadata.fields.season.is_some()
                && metadata.fields.episode.is_some())
            .then(|| id.parse::<u64>().ok())
            .flatten()
        })
        .collect();
    let mut remote = if reviewed.len() == 1 {
        let id = reviewed.iter().next().unwrap();
        let series = auth::digest(&format!("tmdb:series:{id}"));
        unified_catalog::remote_items(&state, identity.user.id, None, Some(&series))
            .await?
            .0
    } else {
        Vec::new()
    };
    enrich_personal(&state, identity.user.id, &mut remote).await?;
    let extra_seasons = unified_catalog::seasons(&remote);
    // Keep the requested local file as the playback and share anchor.
    let anchor = item.as_ref().map(|item| item.file_key.clone());
    let mut all = items.clone();
    all.extend(remote.iter().cloned());
    unified_catalog::dedup_members(&mut all, anchor.as_deref());
    let counts = unified_catalog::seasons(&all);
    let root = media_root(&state, &identity.user, &query.source_id).await?;
    let season = query.season;
    let requested = season.clone();
    let is_series = group.is_some();
    let (item, mut items, artwork, mut seasons) = tokio::task::spawn_blocking(move || {
        let (artwork, seasons) = if is_series {
            sidecars::describe(&root, &items)
        } else {
            Default::default()
        };
        if let Some(key) = &requested {
            items.retain(|item| item.file.kind == "video" && sidecars::season_key(item) == *key);
        }
        // The series overview only needs season artwork. Episode thumbnails are
        // resolved for the selected season, an individual file, or unseasoned groups.
        if requested.is_some()
            || item.is_some()
            || seasons.iter().all(|season| season.key == "unknown")
        {
            sidecars::attach_thumbnails(&root, &mut items);
        }
        if let Some(item) = &mut item {
            sidecars::attach_thumbnails(&root, std::slice::from_mut(item));
        }
        (item, items, artwork, seasons)
    })
    .await
    .map_err(|_| auth::unavailable())?;
    if !remote.is_empty() {
        for candidate in extra_seasons {
            if !seasons.iter().any(|current| current.key == candidate.key) {
                seasons.push(candidate);
            }
        }
        for current in &mut seasons {
            if let Some(total) = counts.iter().find(|count| count.key == current.key) {
                current.count = total.count;
            }
        }
        seasons.sort_by_key(|season| season.key.parse::<u32>().ok());
        if let Some(ref season) = season {
            remote.retain(|item| sidecars::season_key(item) == *season);
        }
        items.extend(remote);
        unified_catalog::dedup_members(&mut items, anchor.as_deref());
    }
    media_root(&state, &identity.user, &query.source_id).await?;
    Ok(Json(LinkContext {
        item,
        members: items,
        label,
        artwork,
        seasons,
        season,
        series_source_id: group.as_ref().map(|_| query.source_id),
        series_group_id: group,
    }))
}
