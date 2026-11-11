//! Per-item discussions use the same access policy as playback and reading.
use crate::{auth, catalog, progress, AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use serde::{Deserialize, Serialize};

const PAGE_SIZE: usize = 30;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Listing {
    source_id: String,
    path: String,
    after: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewComment {
    source_id: String,
    path: String,
    parent_id: Option<String>,
    body: String,
}

#[derive(Serialize, sqlx::FromRow)]
struct Comment {
    id: String,
    parent_id: Option<String>,
    author: String,
    reply_to: Option<String>,
    body: String,
    created_at: f64,
    removed: bool,
}

#[derive(Serialize)]
struct Page {
    comments: Vec<Comment>,
    next: Option<String>,
    can_moderate: bool,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(query): Query<Listing>,
) -> Result<impl IntoResponse, AppError> {
    progress::validate_media(&state, &identity.user, &query.source_id, &query.path).await?;
    let after = query.after.unwrap_or(0);
    if after < 0 {
        return Err(crate::bad_request("Invalid comment cursor."));
    }
    let mut comments = sqlx::query_as::<_, Comment>(
        "SELECT c.id::text, c.parent_id::text, COALESCE(u.username, 'Deleted account') AS author,
                CASE WHEN p.removed_at IS NOT NULL THEN 'Removed comment'
                     WHEN c.parent_id IS NOT NULL THEN COALESCE(pu.username, 'Deleted account') END AS reply_to,
                c.body, EXTRACT(EPOCH FROM c.created_at)::float8 AS created_at, c.removed_at IS NOT NULL AS removed
         FROM media_comments c
         LEFT JOIN users u ON u.id=c.author_id
         LEFT JOIN media_comments p ON p.id=c.parent_id
         LEFT JOIN users pu ON pu.id=p.author_id
         WHERE c.source_id=$1 AND c.file_key=$2 AND c.id>$3
         ORDER BY c.id LIMIT $4",
    )
    .bind(&query.source_id)
    .bind(catalog::file_key(&query.source_id, &query.path))
    .bind(after)
    .bind(PAGE_SIZE as i64 + 1)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    crate::media_root(&state, &identity.user, &query.source_id).await?;
    let next = if comments.len() > PAGE_SIZE {
        comments.pop();
        comments.last().map(|comment| comment.id.clone())
    } else {
        None
    };
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(Page {
            comments,
            next,
            can_moderate: identity.user.is_admin,
        }),
    ))
}

async fn lock_discussion(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    key: &str,
) -> Result<(), AppError> {
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1,0))")
        .bind(format!("comments:{key}"))
        .execute(&mut **tx)
        .await
        .map_err(|_| auth::unavailable())?;
    Ok(())
}

async fn authorized_write<'a>(
    state: &'a AppState,
    user_id: uuid::Uuid,
    source_id: &str,
) -> Result<sqlx::Transaction<'a, sqlx::Postgres>, AppError> {
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    // Lock before checking access again, so a concurrent permission change cannot race the insert.
    sqlx::query("SELECT id FROM sources WHERE id=$1 FOR SHARE")
        .bind(source_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let allowed: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM sources s WHERE s.id=$1 AND
            (s.owner_id=$2 OR EXISTS(SELECT 1 FROM source_permissions p
             WHERE p.source_id=s.id AND p.user_id=$2 AND p.can_discover AND p.can_access_media)))",
    )
    .bind(source_id)
    .bind(user_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if !allowed {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "Media access not granted.".into(),
        ));
    }
    Ok(tx)
}

pub async fn post(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(input): Json<NewComment>,
) -> Result<impl IntoResponse, AppError> {
    progress::validate_media(&state, &identity.user, &input.source_id, &input.path).await?;
    let body = input.body.trim();
    if body.is_empty() || body.chars().count() > 2000 || body.contains('\0') {
        return Err(crate::bad_request("Write a comment of 1–2,000 characters."));
    }
    let parent_id = input
        .parent_id
        .as_deref()
        .map(|id| {
            id.parse::<i64>()
                .ok()
                .filter(|id| *id > 0)
                .ok_or_else(|| crate::bad_request("Invalid parent comment."))
        })
        .transpose()?;
    auth::rate_limit(
        &state.db,
        format!("comments:user:{}", identity.user.id),
        12,
        60,
    )
    .await?;

    let mut tx = authorized_write(&state, identity.user.id, &input.source_id).await?;
    let key = catalog::file_key(&input.source_id, &input.path);
    // Allocate IDs in commit order for each item, so cursor pagination cannot miss a late insert.
    lock_discussion(&mut tx, &key).await?;
    let comment = sqlx::query_as::<_, Comment>(
        "WITH inserted AS (
             INSERT INTO media_comments(source_id,file_key,author_id,parent_id,body)
             SELECT $1,$2,$3,$4,$5 WHERE $4::bigint IS NULL OR EXISTS(
                 SELECT 1 FROM media_comments WHERE source_id=$1 AND file_key=$2 AND id=$4 AND removed_at IS NULL)
             RETURNING *
         )
         SELECT c.id::text,c.parent_id::text,$6::text AS author,
                CASE WHEN p.removed_at IS NOT NULL THEN 'Removed comment'
                     WHEN c.parent_id IS NOT NULL THEN COALESCE(pu.username, 'Deleted account') END AS reply_to,
                c.body, EXTRACT(EPOCH FROM c.created_at)::float8 AS created_at, c.removed_at IS NOT NULL AS removed
         FROM inserted c LEFT JOIN media_comments p ON p.id=c.parent_id
         LEFT JOIN users pu ON pu.id=p.author_id",
    )
    .bind(&input.source_id)
    .bind(key)
    .bind(identity.user.id)
    .bind(parent_id)
    .bind(body)
    .bind(&identity.user.username)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(|| AppError(StatusCode::NOT_FOUND, "Comment not found for this item.".into()))?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok((
        StatusCode::CREATED,
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(comment),
    ))
}

pub async fn remove(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<i64>,
    Query(item): Query<Listing>,
) -> Result<StatusCode, AppError> {
    if !identity.user.is_admin {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Only administrators can remove comments.".into(),
        ));
    }
    progress::validate_media(&state, &identity.user, &item.source_id, &item.path).await?;
    let mut tx = authorized_write(&state, identity.user.id, &item.source_id).await?;
    let key = catalog::file_key(&item.source_id, &item.path);
    lock_discussion(&mut tx, &key).await?;
    let result = sqlx::query(
        "UPDATE media_comments SET
            removed_at=COALESCE(removed_at,now()), removed_by=CASE WHEN removed_at IS NULL THEN $4 ELSE removed_by END,
            author_id=NULL, body='[Comment removed]'
         WHERE source_id=$1 AND file_key=$2 AND id=$3",
    )
    .bind(&item.source_id)
    .bind(key)
    .bind(id)
    .bind(identity.user.id)
    .execute(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if result.rows_affected() == 0 {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "Comment not found for this item.".into(),
        ));
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}
