//! Personal progress is independent of bookmarks and never grants media access.
use crate::{auth, catalog, AppError, AppState};
use axum::{
    extract::{Query, State},
    http::StatusCode,
    Extension, Json,
};
use serde::{Deserialize, Serialize};
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Reference {
    source_id: String,
    relative_path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    source_id: String,
    relative_path: String,
    position: f64,
    total: Option<f64>,
    locator: Option<String>,
    expected_revision: i64,
}
#[derive(Serialize, sqlx::FromRow)]
pub struct Progress {
    position: f64,
    total: Option<f64>,
    locator: Option<String>,
    revision: i64,
}
async fn validate(
    state: &AppState,
    user: &auth::User,
    reference: &Reference,
) -> Result<&'static str, AppError> {
    if reference.source_id.len() > 128 || reference.relative_path.len() > 4096 {
        return Err(crate::bad_request("Invalid media reference."));
    }
    let root = crate::media_root(state, user, &reference.source_id).await?;
    let extension = std::path::Path::new(&reference.relative_path)
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind = crate::classify_extension(&extension)
        .ok_or_else(|| crate::bad_request("Unsupported media type."))?;
    let path = crate::resolve_media_path(&root, &reference.relative_path, kind)?;
    if path.strip_prefix(&root).ok().and_then(|path| path.to_str())
        != Some(reference.relative_path.as_str())
    {
        return Err(crate::bad_request("Use the original catalog path."));
    }
    Ok(kind)
}
fn values(update: &Update, kind: &str) -> Result<(), AppError> {
    if !update.position.is_finite()
        || !(0.0..=31_536_000.0).contains(&update.position)
        || update.total.is_some_and(|total| {
            !total.is_finite() || !(0.0..=31_536_000.0).contains(&total) || total == 0.0
        })
        || update.total.is_some_and(|total| update.position > total)
        || update.expected_revision < 0
        || update.expected_revision == i64::MAX
    {
        return Err(crate::bad_request(
            "Invalid progress position, total, or revision.",
        ));
    }
    if update
        .locator
        .as_ref()
        .is_some_and(|value| value.len() > 4096 || value.contains('\0'))
        || kind != "book" && update.locator.is_some()
        || kind == "book"
            && (update.position.fract() != 0.0
                || update
                    .total
                    .is_some_and(|total| total.fract() != 0.0 || update.position >= total))
    {
        return Err(crate::bad_request(
            "Invalid book location or media progress unit.",
        ));
    }
    Ok(())
}
pub async fn get(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(reference): Query<Reference>,
) -> Result<Json<Option<Progress>>, AppError> {
    validate(&state, &identity.user, &reference).await?;
    let row=sqlx::query_as::<_,Progress>("SELECT position,total,locator,revision FROM media_progress WHERE user_id=$1 AND file_key=$2").bind(identity.user.id).bind(catalog::file_key(&reference.source_id,&reference.relative_path)).fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?;
    Ok(Json(row))
}
pub async fn put(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(update): Json<Update>,
) -> Result<Json<Progress>, AppError> {
    let reference = Reference {
        source_id: update.source_id.clone(),
        relative_path: update.relative_path.clone(),
    };
    let kind = validate(&state, &identity.user, &reference).await?;
    values(&update, kind)?;
    auth::rate_limit(
        &state.db,
        format!("progress:user:{}", identity.user.id),
        120,
        60,
    )
    .await?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    // Permission writers lock the source, so revocation and progress writes serialize.
    let allowed:Option<String>=sqlx::query_scalar("SELECT s.id FROM sources s WHERE s.id=$2 AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media)) FOR SHARE OF s").bind(identity.user.id).bind(&update.source_id).fetch_optional(&mut *tx).await.map_err(|_|auth::unavailable())?;
    if allowed.is_none() {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "Media access not granted.".into(),
        ));
    }
    let key = catalog::file_key(&update.source_id, &update.relative_path);
    // Serialize first writes as well as updates, without creating personal library state.
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("progress:{}:{key}", identity.user.id))
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let revision: Option<i64> =
        sqlx::query_scalar("SELECT revision FROM media_progress WHERE user_id=$1 AND file_key=$2")
            .bind(identity.user.id)
            .bind(&key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    if revision.unwrap_or(0) != update.expected_revision {
        return Err(AppError(
            StatusCode::CONFLICT,
            "Progress changed in another player. Reload progress before saving again.".into(),
        ));
    }
    let row=sqlx::query_as::<_,Progress>("INSERT INTO media_progress(user_id,file_key,source_id,relative_path,position,total,locator,revision) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT(user_id,file_key) DO UPDATE SET position=$5,total=$6,locator=$7,revision=$8,updated_at=now() RETURNING position,total,locator,revision").bind(identity.user.id).bind(key).bind(update.source_id).bind(update.relative_path).bind(update.position).bind(update.total).bind(update.locator).bind(update.expected_revision+1).fetch_one(&mut *tx).await.map_err(|_|auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(row))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_media_units_and_bounded_positions() {
        let mut update = Update {
            source_id: "source".into(),
            relative_path: "file".into(),
            position: 10.5,
            total: Some(20.0),
            locator: None,
            expected_revision: 0,
        };
        assert!(values(&update, "video").is_ok());
        assert!(values(&update, "book").is_err());
        update.position = 20.0;
        assert!(values(&update, "book").is_err());
        update.position = 3.0;
        assert!(values(&update, "book").is_ok());
        update.position = f64::NAN;
        assert!(values(&update, "video").is_err());
    }
}

#[derive(Clone, Serialize, sqlx::FromRow)]
pub struct Summary {
    pub position: f64,
    pub total: Option<f64>,
    pub updated_at: f64,
}
#[derive(sqlx::FromRow)]
struct SummaryRow {
    file_key: String,
    position: f64,
    total: Option<f64>,
    updated_at: f64,
}
pub async fn enrich(
    state: &AppState,
    user: uuid::Uuid,
    items: &mut [catalog::Item],
) -> Result<(), AppError> {
    let keys: Vec<_> = items.iter().map(|item| item.file_key.as_str()).collect();
    let rows=sqlx::query_as::<_,SummaryRow>("SELECT p.file_key,p.position,p.total,extract(epoch from p.updated_at)::float8 AS updated_at FROM media_progress p JOIN sources s ON s.id=p.source_id WHERE p.user_id=$1 AND p.file_key=ANY($2) AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions a WHERE a.source_id=s.id AND a.user_id=$1 AND a.can_discover AND a.can_access_media))").bind(user).bind(keys).fetch_all(&state.db).await.map_err(|_|auth::unavailable())?;
    let mut rows: std::collections::HashMap<_, _> = rows
        .into_iter()
        .map(|row| {
            (
                row.file_key,
                Summary {
                    position: row.position,
                    total: row.total,
                    updated_at: row.updated_at,
                },
            )
        })
        .collect();
    for item in items {
        item.progress = rows.remove(&item.file_key);
    }
    Ok(())
}
