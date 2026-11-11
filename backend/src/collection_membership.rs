//! Source-owned collection associations, independent of logical file identity.
use crate::{auth, catalog, unified_catalog, AppError, AppState};
use axum::{
    extract::{DefaultBodyLimit, State},
    http::StatusCode,
    routing::post,
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use uuid::Uuid;

#[derive(Clone, Serialize, FromRow)]
pub(crate) struct Collection {
    #[serde(skip)]
    location_id: Uuid,
    pub source_id: String,
    pub group_id: String,
    pub label: String,
}

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/catalog/v1/collection-membership", post(update))
        .layer(DefaultBodyLimit::max(4096))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    locations: Vec<Uuid>,
    target: Option<Uuid>,
    group: Option<String>,
}

const VISIBLE: &str = "FROM catalog_collection_memberships m
    JOIN catalog_discoverable_locations own ON own.location_id=m.location_id
        AND own.representation_id=m.representation_id AND own.viewer_id=$1
    JOIN catalog_discoverable_locations target ON target.location_id=m.target_location_id
        AND target.representation_id=m.target_representation_id AND target.viewer_id=$1
        AND (target.source_id LIKE 'remote:%' OR target.playback_eligible)";

async fn update(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(change): Json<Change>,
) -> Result<StatusCode, AppError> {
    let viewer = identity.user.id;
    if change.locations.is_empty() || change.locations.len() > 50 {
        return Err(crate::bad_request("Select 1–50 files."));
    }
    let mut locations = change.locations;
    locations.sort_unstable();
    locations.dedup();
    auth::rate_limit(&state.db, format!("collection:user:{viewer}"), 24, 300).await?;
    let target = match (change.target, change.group) {
        (Some(id), Some(group)) if !locations.contains(&id) => {
            let row: (String,Uuid) = sqlx::query_as(
                "SELECT source_id,representation_id FROM catalog_discoverable_locations WHERE viewer_id=$1 AND location_id=$2",
            ).bind(viewer).bind(id).fetch_optional(&state.db).await
              .map_err(|_|auth::unavailable())?.ok_or_else(unified_catalog::missing)?;
            let items = if row.0.starts_with(unified_catalog::REMOTE_PREFIX) {
                unified_catalog::remote_items(&state, viewer, Some(&[row.1]), None)
                    .await?
                    .0
            } else {
                catalog::source_items(&state, &identity.user, &row.0).await?
            };
            let item = items
                .into_iter()
                .find(|item| item.location_id == Some(id))
                .ok_or_else(unified_catalog::missing)?;
            let group = item
                .metadata
                .and_then(|metadata| metadata.group)
                .filter(|value| value.id == group)
                .ok_or_else(|| crate::bad_request("Select an existing series or album."))?;
            Some((id, row.1, item.file.kind, group))
        }
        (None, None) => None,
        _ => return Err(crate::bad_request("Select a different collection anchor.")),
    };
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    let owned: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT v.location_id,v.representation_id,i.kind
         FROM catalog_discoverable_locations v JOIN sources s ON s.id=v.source_id AND s.owner_id=$1
         JOIN catalog_items i ON i.id=v.item_id WHERE v.viewer_id=$1 AND v.location_id=ANY($2)
         FOR SHARE OF s",
    )
    .bind(viewer)
    .bind(&locations)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if owned.len() != locations.len() {
        return Err(unified_catalog::missing());
    }
    if let Some((id, rep, kind, group)) = target {
        if owned.iter().any(|row| row.2 != kind) {
            return Err(crate::bad_request(
                "Select files of the collection’s media type.",
            ));
        }
        let (ids, reps): (Vec<_>, Vec<_>) = owned.into_iter().map(|(id, rep, _)| (id, rep)).unzip();
        let changed=sqlx::query(
            "INSERT INTO catalog_collection_memberships(location_id,representation_id,target_location_id,target_representation_id,target_group,label)
             SELECT own.id,own.rep,$3,$4,$5,$6
             FROM unnest($1::uuid[],$2::uuid[]) own(id,rep)
             WHERE EXISTS(SELECT 1 FROM catalog_discoverable_locations
                WHERE location_id=$3 AND representation_id=$4 AND viewer_id=$7)
             ON CONFLICT(location_id) DO UPDATE SET representation_id=excluded.representation_id,
                target_location_id=excluded.target_location_id,target_representation_id=excluded.target_representation_id,
                target_group=excluded.target_group,label=excluded.label",
        ).bind(ids).bind(reps).bind(id).bind(rep).bind(group.id).bind(group.label).bind(viewer)
         .execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
        if changed.rows_affected() != locations.len() as u64 {
            return Err(unified_catalog::missing());
        }
    } else {
        sqlx::query("DELETE FROM catalog_collection_memberships WHERE location_id=ANY($1)")
            .bind(locations)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn annotate(
    state: &AppState,
    viewer: Uuid,
    items: &mut [catalog::Item],
) -> Result<(), AppError> {
    let ids: Vec<_> = items.iter().filter_map(|item| item.location_id).collect();
    if ids.is_empty() {
        return Ok(());
    }
    let sql = format!(
        "SELECT m.location_id,target.source_id,m.target_group AS group_id,m.label {VISIBLE}
        WHERE m.location_id=ANY($2)"
    );
    let mut links: std::collections::HashMap<_, _> = sqlx::query_as::<_, Collection>(&sql)
        .bind(viewer)
        .bind(ids)
        .fetch_all(&state.db)
        .await
        .map_err(|_| auth::unavailable())?
        .into_iter()
        .map(|link| (link.location_id, link))
        .collect();
    for item in items {
        item.collection_link = item.location_id.and_then(|id| links.remove(&id));
    }
    Ok(())
}

pub(crate) async fn extend(
    state: &AppState,
    identity: &auth::Identity,
    source: &str,
    context: &mut catalog::LinkContext,
) -> Result<(), AppError> {
    if let Some(item) = context.item.as_mut() {
        annotate(state, identity.user.id, std::slice::from_mut(item)).await?;
    }
    let group = context
        .item
        .as_ref()
        .and_then(|item| item.collection_link.clone())
        .or_else(|| {
            Some(Collection {
                location_id: Uuid::nil(),
                source_id: source.into(),
                group_id: context.series_group_id.clone()?,
                label: context.label.clone()?,
            })
        });
    let Some(group) = group else {
        return Ok(());
    };
    let sql = format!(
        "SELECT EXISTS(SELECT 1 {VISIBLE} WHERE target.source_id=$2 AND m.target_group=$3)"
    );
    let has_members: bool = sqlx::query_scalar(&sql)
        .bind(identity.user.id)
        .bind(&group.source_id)
        .bind(&group.group_id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    if !has_members {
        return Ok(());
    }
    let catalog = catalog::discover(State(state.clone()), Extension(identity.clone()))
        .await?
        .0;
    let mut all = if context
        .item
        .as_ref()
        .is_some_and(|item| item.collection_link.is_some())
    {
        context.item.iter().cloned().collect()
    } else {
        context.members.clone()
    };
    all.extend(catalog.items.into_iter().filter(|item| {
        item.collection_link.as_ref().is_some_and(|link| {
            link.source_id == group.source_id && link.group_id == group.group_id
        }) || item.source_id == group.source_id
            && item
                .metadata
                .as_ref()
                .and_then(|m| m.group.as_ref())
                .is_some_and(|g| g.id == group.group_id)
    }));
    unified_catalog::dedup_members(
        &mut all,
        context.item.as_ref().map(|item| item.file_key.as_str()),
    );
    let seasons = unified_catalog::seasons(&all);
    for entry in seasons {
        if let Some(existing) = context
            .seasons
            .iter_mut()
            .find(|current| current.key == entry.key)
        {
            existing.count = entry.count;
        } else {
            context.seasons.push(entry);
        }
    }
    context
        .seasons
        .sort_by_key(|entry| entry.key.parse::<u32>().ok());
    if let Some(season) = context.season.as_ref() {
        all.retain(|item| crate::metadata::sidecars::season_key(item) == *season);
    }
    context.members = all;
    Ok(())
}
