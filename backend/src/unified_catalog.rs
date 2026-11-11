//! Compatibility adapter from permitted inventory locations to existing page items.
use crate::{
    auth,
    catalog::{self, Item},
    metadata::{
        self,
        groups::GroupView,
        model::Fields,
        sidecars::{self, Season},
    },
    AppError, AppState, MediaFile,
};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension,
};
use sqlx::{FromRow, PgConnection};
use std::collections::{BTreeMap, HashMap, HashSet};
use uuid::Uuid;

pub(crate) const REMOTE_PREFIX: &str = "remote:";
const REMOTE_LIMIT: i64 = 5_000;

pub(crate) fn missing() -> AppError {
    AppError(
        StatusCode::NOT_FOUND,
        "Media is unavailable or access has not been granted.".into(),
    )
}

#[derive(FromRow)]
struct RemoteRow {
    location_id: Uuid,
    representation_id: Uuid,
    remote_representation_id: Uuid,
    logical_id: Uuid,
    source_id: String,
    source_name: String,
    name: String,
    extension: String,
    size_bytes: i64,
    playback_eligible: bool,
    media_granted: bool,
    namespace: Option<String>,
    identifier: Option<String>,
    provider_fields: Option<serde_json::Value>,
    provider_art: Option<String>,
    series_group: Option<String>,
    owner: bool,
    custom_fields: Option<serde_json::Value>,
    custom_group: Option<String>,
    hide_artwork: bool,
}

impl RemoteRow {
    fn into_item(self) -> Option<Item> {
        let kind = crate::classify_extension(&self.extension)?;
        let source = self.source_id.strip_prefix(REMOTE_PREFIX)?;
        let path = self.representation_id.to_string();
        let file_key = catalog::file_key(&self.source_id, &path);
        let stream = format!(
            "/api/nodes/v1/sources/{source}/representations/{}?version={}",
            self.remote_representation_id, self.representation_id
        );
        let mut fields: Fields = self
            .provider_fields
            .and_then(|fields| serde_json::from_value(fields).ok())
            .unwrap_or_default();
        let provider = self.namespace;
        let group = self.series_group.and_then(|id| {
            let mut parts = self.identifier.as_deref()?.split(':');
            let series = parts.next()?;
            fields.season = parts.next()?.parse().ok();
            fields.episode = parts.next()?.parse().ok();
            let label = fields
                .collection
                .clone()
                .unwrap_or_else(|| format!("Series {series}"));
            fields.collection = Some(label.clone());
            Some(GroupView {
                id,
                label,
                can_edit: false,
                has_custom_genres: false,
                has_custom_artwork: false,
                synopsis: None,
                synopsis_provider: None,
            })
        });
        let has_custom = self.custom_fields.is_some();
        if let Some(custom) = self
            .custom_fields
            .and_then(|value| serde_json::from_value(value).ok())
        {
            fields = custom;
        }
        let group = group
            .map(|mut group| {
                if let Some(label) = fields
                    .collection
                    .as_ref()
                    .filter(|label| !label.trim().is_empty())
                {
                    group.label = label.clone();
                }
                group
            })
            .or_else(|| {
                self.custom_group
                    .and_then(|_| metadata::groups::identity(&self.source_id, kind, &fields))
            });
        let artwork_url = self
            .provider_art
            .filter(|_| !self.hide_artwork)
            .map(|_| format!("/api/catalog/v1/artwork/{}", self.representation_id));
        let has_artwork = artwork_url.is_some();
        let metadata = (provider.is_some() || has_custom || self.owner).then(|| metadata::View {
            group,
            fields,
            artwork_url,
            origin: if has_custom {
                "custom"
            } else if provider.is_some() {
                "provider_curator"
            } else {
                "agent"
            }
            .into(),
            individual_genres: None,
            credits: match provider.as_deref() {
                Some("tmdb_movie" | "tmdb_tv") if !has_custom || has_artwork => vec!["tmdb".into()],
                Some("musicbrainz") if has_artwork => {
                    vec!["musicbrainz".into(), "coverartarchive".into()]
                }
                Some(provider) if !has_custom || has_artwork => vec![provider.into()],
                _ => vec![],
            },
            has_custom,
            thumbnail_url: None,
            can_edit: self.owner,
            artwork_preference: if self.hide_artwork {
                metadata::model::ArtworkPreference::None
            } else {
                Default::default()
            },
            provider,
            provider_id: self.identifier,
        });
        Some(Item {
            file_key,
            source_id: self.source_id,
            source_name: self.source_name,
            file: MediaFile {
                name: self.name,
                relative_path: path,
                kind,
                extension: self.extension,
                size_bytes: self.size_bytes.try_into().ok()?,
                modified_at: None,
            },
            saved_at: None,
            last_opened_at: None,
            available: self.playback_eligible,
            progress: None,
            metadata,
            logical_id: Some(self.logical_id),
            location_id: Some(self.location_id),
            collection_link: None,
            remote: true,
            media_url: self.playback_eligible.then_some(stream),
            remote_access: Some(self.media_granted),
        })
    }
}

pub(crate) async fn remote_items(
    state: &AppState,
    viewer: Uuid,
    representations: Option<&[Uuid]>,
    series: Option<&str>,
) -> Result<(Vec<Item>, bool), AppError> {
    let mut rows = sqlx::query_as::<_, RemoteRow>(
        "SELECT v.*,s.owner_id=$1 AS owner,o.fields AS custom_fields,
                o.group_id AS custom_group,COALESCE(o.hide_artwork,false) AS hide_artwork
         FROM catalog_remote_page_locations v JOIN sources s ON s.id=v.source_id
         LEFT JOIN catalog_remote_metadata_overrides o ON o.representation_id=v.representation_id
         WHERE v.viewer_id=$1 AND ($2::uuid[] IS NULL OR v.representation_id=ANY($2))
           AND ($3::text IS NULL OR COALESCE(v.series_group,o.group_id)=$3)
         ORDER BY v.logical_id,v.playback_eligible DESC,v.location_id LIMIT $4",
    )
    .bind(viewer)
    .bind(representations)
    .bind(series)
    .bind(REMOTE_LIMIT + 1)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    let truncated = rows.len() > REMOTE_LIMIT as usize;
    rows.truncate(REMOTE_LIMIT as usize);
    Ok((
        rows.into_iter().filter_map(RemoteRow::into_item).collect(),
        truncated,
    ))
}

pub(crate) async fn annotate_local(
    state: &AppState,
    viewer: Uuid,
    items: &mut [Item],
) -> Result<(), AppError> {
    if items.is_empty() {
        return Ok(());
    }
    let keys: Vec<_> = items.iter().map(|item| item.file_key.as_str()).collect();
    let ids: HashMap<String, (Uuid, Uuid)> = sqlx::query_as::<_, (String, Uuid, Uuid)>(
        "SELECT l.local_file_key,v.logical_id,l.id FROM catalog_locations l
         JOIN catalog_matched_locations v ON v.location_id=l.id
         WHERE v.viewer_id=$1 AND l.local_file_key=ANY($2)",
    )
    .bind(viewer)
    .bind(keys)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?
    .into_iter()
    .map(|(key, logical, location)| (key, (logical, location)))
    .collect();
    for item in items {
        if let Some(&(logical, location)) = ids.get(&item.file_key) {
            item.logical_id = Some(logical);
            item.location_id = Some(location);
        }
    }
    Ok(())
}

pub(crate) async fn authorized(
    db: &mut PgConnection,
    viewer: Uuid,
    source: &str,
    path: &str,
    require_online: bool,
) -> Result<&'static str, AppError> {
    let id = Uuid::parse_str(path).map_err(|_| missing())?;
    if id.to_string() != path {
        return Err(missing());
    }
    let extension: Option<String> = sqlx::query_scalar(
        "SELECT extension FROM catalog_remote_page_locations WHERE viewer_id=$1 AND source_id=$2
         AND representation_id=$3 AND media_granted AND (NOT $4 OR playback_eligible)",
    )
    .bind(viewer)
    .bind(source)
    .bind(id)
    .bind(require_online)
    .fetch_optional(db)
    .await
    .map_err(|_| auth::unavailable())?;
    extension
        .as_deref()
        .and_then(crate::classify_extension)
        .ok_or_else(missing)
}

pub(crate) async fn artwork(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(representation): Path<Uuid>,
) -> Result<Response, AppError> {
    let bytes: Vec<u8> = sqlx::query_scalar(
        "SELECT a.jpeg FROM catalog_remote_page_locations v JOIN metadata_artwork a ON a.id=v.provider_art
         WHERE v.viewer_id=$1 AND v.representation_id=$2 AND NOT EXISTS(
             SELECT 1 FROM catalog_remote_metadata_overrides o WHERE o.representation_id=v.representation_id AND o.hide_artwork)",
    )
    .bind(identity.user.id).bind(representation).fetch_optional(&state.db)
    .await.map_err(|_|auth::unavailable())?.ok_or_else(missing)?;
    Ok((
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

pub(crate) fn dedup_members(items: &mut Vec<Item>, anchor: Option<&str>) {
    let mut seen = HashSet::new();
    items.sort_by_key(|item| {
        (
            anchor != Some(item.file_key.as_str()),
            !item.available,
            item.remote,
            item.file_key.clone(),
        )
    });
    items.retain(|item| {
        seen.insert(
            item.logical_id
                .map_or_else(|| item.file_key.clone(), |id| id.to_string()),
        )
    });
    items.sort_by(|a, b| {
        let position = |item: &Item| {
            let fields = item.metadata.as_ref().map(|m| &m.fields);
            (
                fields.and_then(|f| f.season),
                fields.and_then(|f| f.episode),
            )
        };
        position(a)
            .cmp(&position(b))
            .then_with(|| natord::compare(&a.file.name, &b.file.name))
    });
}

pub(crate) fn seasons(items: &[Item]) -> Vec<Season> {
    let mut counts = BTreeMap::new();
    for item in items.iter().filter(|item| item.file.kind == "video") {
        let season = item.metadata.as_ref().and_then(|m| m.fields.season);
        *counts.entry(season).or_insert(0usize) += 1;
    }
    counts
        .into_iter()
        .map(|(number, count)| Season {
            key: number.map_or_else(|| "unknown".into(), |number| number.to_string()),
            count,
            poster_url: None,
        })
        .collect()
}

pub(crate) async fn remote_link(
    state: &AppState,
    viewer: Uuid,
    source: String,
    path: Option<String>,
    group: Option<String>,
    season: Option<String>,
) -> Result<catalog::LinkContext, AppError> {
    let mut item = if let Some(path) = path {
        let id = Uuid::parse_str(&path).map_err(|_| missing())?;
        let (mut items, _) = remote_items(state, viewer, Some(&[id]), None).await?;
        let selected = items
            .pop()
            .filter(|item| item.source_id == source && item.file.relative_path == path)
            .ok_or_else(missing)?;
        Some(selected)
    } else {
        None
    };
    let group = group.or_else(|| {
        item.as_ref()?
            .metadata
            .as_ref()?
            .group
            .as_ref()
            .map(|g| g.id.clone())
    });
    let mut members = if let Some(ref group) = group {
        let allowed: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM catalog_remote_page_locations
             WHERE viewer_id=$1 AND source_id=$2 AND (series_group=$3 OR (series_group IS NULL AND EXISTS(
                SELECT 1 FROM catalog_remote_metadata_overrides o
                WHERE o.representation_id=catalog_remote_page_locations.representation_id AND o.group_id=$3))))",
        )
        .bind(viewer)
        .bind(&source)
        .bind(group)
        .fetch_one(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
        if !allowed {
            return Err(missing());
        }
        remote_items(state, viewer, None, Some(group)).await?.0
    } else {
        item.iter().cloned().collect()
    };
    catalog::enrich_personal(state, viewer, &mut members).await?;
    if let Some(ref mut selected) = item {
        *selected = members
            .iter()
            .find(|m| m.file_key == selected.file_key)
            .cloned()
            .ok_or_else(missing)?;
    }
    dedup_members(
        &mut members,
        item.as_ref().map(|item| item.file_key.as_str()),
    );
    let seasons = seasons(&members);
    let first = members.first().ok_or_else(missing)?;
    let label = first
        .metadata
        .as_ref()
        .and_then(|m| m.group.as_ref())
        .map(|group| group.label.clone());
    if let Some(ref season) = season {
        members.retain(|item| sidecars::season_key(item) == *season);
    }
    Ok(catalog::LinkContext {
        item,
        members,
        label,
        artwork: Default::default(),
        seasons,
        season,
        series_source_id: group.as_ref().map(|_| source),
        series_group_id: group,
    })
}

pub(crate) fn metadata_routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/api/catalog/v1/remote-metadata/{id}",
            axum::routing::get(owner_metadata)
                .put(save_owner_metadata)
                .delete(reset_owner_metadata),
        )
        .layer(axum::extract::DefaultBodyLimit::max(32 * 1024))
}

async fn owner_item(state: &AppState, user: Uuid, id: Uuid) -> Result<Item, AppError> {
    let (items, _) = remote_items(state, user, Some(&[id]), None).await?;
    items
        .into_iter()
        .find(|item| item.metadata.as_ref().is_some_and(|view| view.can_edit))
        .ok_or_else(missing)
}

fn metadata_response(item: Item) -> Result<Response, AppError> {
    Ok((
        [("cache-control", "private, no-store")],
        axum::Json(item.metadata.ok_or_else(missing)?),
    )
        .into_response())
}

async fn owner_metadata(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    metadata_response(owner_item(&state, identity.user.id, id).await?)
}

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct OwnerEdit {
    path: String,
    fields: Fields,
    artwork_preference: metadata::model::ArtworkPreference,
}

const OWNER_CHECK: &str = "SELECT v.representation_id FROM catalog_remote_page_locations v
    JOIN sources s ON s.id=v.source_id AND s.owner_id=$1 WHERE v.viewer_id=$1 AND v.representation_id=$2";

async fn save_owner_metadata(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    axum::Json(edit): axum::Json<OwnerEdit>,
) -> Result<Response, AppError> {
    let user = identity.user.id;
    let item = owner_item(&state, user, id).await?;
    if edit.path != item.file.relative_path {
        return Err(missing());
    }
    auth::rate_limit(&state.db, format!("remote-metadata:{user}"), 60, 300).await?;
    let fields = edit.fields.normalize()?;
    let group =
        metadata::groups::identity(&item.source_id, item.file.kind, &fields).map(|group| group.id);
    let data = serde_json::to_value(fields).map_err(|_| auth::unavailable())?;
    let sql=format!("INSERT INTO catalog_remote_metadata_overrides(representation_id,fields,group_id,hide_artwork)
        SELECT eligible.representation_id,$3,$4,$5 FROM ({OWNER_CHECK}) eligible
        ON CONFLICT(representation_id) DO UPDATE SET fields=excluded.fields,group_id=excluded.group_id,
            hide_artwork=excluded.hide_artwork");
    if sqlx::query(&sql)
        .bind(user)
        .bind(id)
        .bind(data)
        .bind(group)
        .bind(edit.artwork_preference == metadata::model::ArtworkPreference::None)
        .execute(&state.db)
        .await
        .map_err(|_| auth::unavailable())?
        .rows_affected()
        != 1
    {
        return Err(missing());
    }
    metadata_response(owner_item(&state, user, id).await?)
}

async fn reset_owner_metadata(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let user = identity.user.id;
    owner_item(&state, user, id).await?;
    auth::rate_limit(&state.db, format!("remote-metadata:{user}"), 60, 300).await?;
    let sql = format!(
        "DELETE FROM catalog_remote_metadata_overrides WHERE representation_id IN ({OWNER_CHECK})"
    );
    sqlx::query(&sql)
        .bind(user)
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    metadata_response(owner_item(&state, user, id).await?)
}
