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
}

impl RemoteRow {
    fn into_item(self) -> Option<Item> {
        let kind = crate::classify_extension(&self.extension)?;
        let source = self.source_id.strip_prefix(REMOTE_PREFIX)?;
        let path = self.representation_id.to_string();
        let file_key = catalog::file_key(&self.source_id, &path);
        let stream = format!(
            "/api/nodes/v1/sources/{source}/representations/{}",
            self.remote_representation_id
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
        let has_artwork = self.provider_art.is_some();
        let artwork_url = self
            .provider_art
            .map(|_| format!("/api/catalog/v1/artwork/{}", self.representation_id));
        let metadata = provider.as_ref().map(|provider| metadata::View {
            group,
            fields,
            artwork_url,
            origin: "provider_curator".into(),
            individual_genres: None,
            credits: match provider.as_str() {
                "tmdb_movie" | "tmdb_tv" => vec!["tmdb".into()],
                "musicbrainz" if has_artwork => {
                    vec!["musicbrainz".into(), "coverartarchive".into()]
                }
                _ => vec![provider.clone()],
            },
            has_custom: false,
            thumbnail_url: None,
            can_edit: false,
            artwork_preference: Default::default(),
            provider: Some(provider.clone()),
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
        "SELECT * FROM catalog_remote_page_locations
         WHERE viewer_id=$1 AND ($2::uuid[] IS NULL OR representation_id=ANY($2))
           AND ($3::text IS NULL OR series_group=$3)
         ORDER BY logical_id,playback_eligible DESC,location_id LIMIT $4",
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
    let ids: HashMap<String, Uuid> = sqlx::query_as(
        "SELECT l.local_file_key,v.logical_id FROM catalog_locations l
         JOIN catalog_matched_locations v ON v.location_id=l.id
         WHERE v.viewer_id=$1 AND l.local_file_key=ANY($2)",
    )
    .bind(viewer)
    .bind(keys)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?
    .into_iter()
    .collect();
    for item in items {
        item.logical_id = ids.get(&item.file_key).copied();
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
         WHERE v.viewer_id=$1 AND v.representation_id=$2",
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
             WHERE viewer_id=$1 AND source_id=$2 AND series_group=$3)",
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
    if let Some(ref season) = season {
        members.retain(|item| sidecars::season_key(item) == *season);
    }
    let first = members.first().ok_or_else(missing)?;
    let label = first
        .metadata
        .as_ref()
        .and_then(|m| m.group.as_ref())
        .map(|group| group.label.clone());
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
