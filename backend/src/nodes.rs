//! Coordinator endpoints for owner-bound Source Agent identity.
use crate::{auth, security::ClientIp, AppError, AppState};
use axum::{
    extract::{Path, State},
    http::{header, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use grimoire_api::node_protocol::{self, Enrollment, Heartbeat, Rotation};
use rand::RngCore;
use serde::Serialize;
use uuid::Uuid;

const PRIVATE: [(header::HeaderName, &str); 1] = [(header::CACHE_CONTROL, "private, no-store")];

pub(crate) fn rejected() -> AppError {
    AppError(
        StatusCode::UNAUTHORIZED,
        "Node credentials or request rejected.".into(),
    )
}

pub(crate) fn version(version: u32) -> Result<(), AppError> {
    if version == node_protocol::VERSION {
        Ok(())
    } else {
        Err(AppError(
            StatusCode::UPGRADE_REQUIRED,
            "Unsupported node protocol version.".into(),
        ))
    }
}

#[derive(Serialize)]
struct EnrollmentToken {
    token: String,
    expires_in: u16,
}

pub async fn issue(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<impl IntoResponse, AppError> {
    let owner = identity.user.id;
    auth::rate_limit(&state.db, format!("nodes:issue:{owner}"), 6, 600).await?;
    let mut random = [0u8; 32];
    rand::rngs::OsRng.fill_bytes(&mut random);
    let token = hex::encode(random);
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    sqlx::query("DELETE FROM source_node_enrollments WHERE expires_at<=now()")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    sqlx::query(
        "INSERT INTO source_node_enrollments(token_hash,owner_id,expires_at)
                 VALUES($1,$2,now()+interval '10 minutes')",
    )
    .bind(auth::digest(&token))
    .bind(owner)
    .execute(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok((
        StatusCode::CREATED,
        PRIVATE,
        Json(EnrollmentToken {
            token,
            expires_in: 600,
        }),
    ))
}

pub async fn enroll(
    State(state): State<AppState>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Json(input): Json<Enrollment>,
) -> Result<StatusCode, AppError> {
    version(input.version)?;
    auth::rate_limit(&state.db, format!("nodes:enroll:{ip}"), 20, 60).await?;
    let key = node_protocol::decode_hex::<32>(&input.public_key).ok_or_else(rejected)?;
    if input.node_id.is_nil()
        || node_protocol::decode_hex::<32>(&input.token).is_none()
        || !(1..=80).contains(&input.label.chars().count())
        || input.label.trim() != input.label
        || input.label.chars().any(char::is_control)
        || !node_protocol::verify(&key, &input.signed_bytes(), &input.signature)
    {
        return Err(rejected());
    }
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    let owner: Uuid = sqlx::query_scalar(
        "DELETE FROM source_node_enrollments e USING users u
         WHERE e.token_hash=$1 AND e.expires_at>now() AND u.id=e.owner_id AND NOT u.disabled
         RETURNING e.owner_id",
    )
    .bind(auth::digest(&input.token))
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(rejected)?;
    let created = sqlx::query(
        "INSERT INTO source_nodes(id,owner_id,label,public_key,last_seen)
         VALUES($1,$2,$3,$4,now())
         ON CONFLICT (id) DO UPDATE
         SET label=EXCLUDED.label,public_key=EXCLUDED.public_key,last_seen=now(),
             last_request_ms=NULL,revoked_at=NULL
         WHERE source_nodes.owner_id=EXCLUDED.owner_id
           AND source_nodes.revoked_at IS NOT NULL
           AND source_nodes.public_key IS DISTINCT FROM EXCLUDED.public_key",
    )
    .bind(input.node_id)
    .bind(owner)
    .bind(input.label)
    .bind(key.as_slice())
    .execute(&mut *tx)
    .await
    .map_err(|error| {
        if error
            .as_database_error()
            .is_some_and(|error| error.is_unique_violation())
        {
            rejected()
        } else {
            auth::unavailable()
        }
    })?
    .rows_affected();
    if created != 1 {
        return Err(rejected());
    }
    // Require the replacement identity to confirm its current catalog and policy.
    sqlx::query(
        "UPDATE remote_catalog_state c SET last_confirmed='-infinity'
                 FROM sources s WHERE s.id=c.source_id AND s.node_id=$1",
    )
    .bind(input.node_id)
    .execute(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    sqlx::query("INSERT INTO source_node_events(node_id,actor_id,event) VALUES($1,$2,'enrolled')")
        .bind(input.node_id)
        .bind(owner)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::CREATED)
}

pub async fn heartbeat(
    State(state): State<AppState>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Json(input): Json<Heartbeat>,
) -> Result<StatusCode, AppError> {
    version(input.version)?;
    auth::rate_limit(&state.db, format!("nodes:heartbeat:{ip}"), 180, 60).await?;
    if input.timestamp_ms.abs_diff(node_protocol::now_ms()) > node_protocol::CLOCK_SKEW_MS {
        return Err(rejected());
    }
    let key = active_key(&state, input.node_id).await?;
    if !node_protocol::verify(&key, &input.signed_bytes(), &input.signature) {
        return Err(rejected());
    }
    let accepted = sqlx::query(
        "UPDATE source_nodes n SET last_seen=now(),last_request_ms=$2 FROM users u
         WHERE n.id=$1 AND n.public_key=$3 AND n.revoked_at IS NULL AND u.id=n.owner_id
         AND NOT u.disabled AND (n.last_request_ms IS NULL OR n.last_request_ms<$2)",
    )
    .bind(input.node_id)
    .bind(input.timestamp_ms)
    .bind(key)
    .execute(&state.db)
    .await
    .map_err(|_| auth::unavailable())?
    .rows_affected();
    if accepted != 1 {
        return Err(rejected());
    }
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn active_key(state: &AppState, id: Uuid) -> Result<Vec<u8>, AppError> {
    sqlx::query_scalar(
        "SELECT n.public_key FROM source_nodes n JOIN users u ON u.id=n.owner_id
         WHERE n.id=$1 AND n.revoked_at IS NULL AND n.public_key IS NOT NULL AND NOT u.disabled",
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(rejected)
}

pub async fn rotate(
    State(state): State<AppState>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    Json(input): Json<Rotation>,
) -> Result<StatusCode, AppError> {
    version(input.version)?;
    auth::rate_limit(&state.db, format!("nodes:rotate:{ip}"), 20, 60).await?;
    if input.timestamp_ms.abs_diff(node_protocol::now_ms()) > node_protocol::CLOCK_SKEW_MS {
        return Err(rejected());
    }
    let old_key = active_key(&state, input.node_id).await?;
    let new_key = node_protocol::decode_hex::<32>(&input.new_public_key).ok_or_else(rejected)?;
    let message = input.signed_bytes();
    if old_key == new_key
        || !node_protocol::verify(&old_key, &message, &input.signature)
        || !node_protocol::verify(&new_key, &message, &input.new_signature)
    {
        return Err(rejected());
    }
    let result = sqlx::query(
        "WITH rotated AS (
            UPDATE source_nodes n SET public_key=$4,last_seen=now(),last_request_ms=$2 FROM users u
            WHERE n.id=$1 AND n.public_key=$3 AND n.revoked_at IS NULL AND u.id=n.owner_id
              AND NOT u.disabled AND (n.last_request_ms IS NULL OR n.last_request_ms<$2)
            RETURNING n.id,n.owner_id
         )
         INSERT INTO source_node_events(node_id,actor_id,event) SELECT id,owner_id,'rotated' FROM rotated",
    )
    .bind(input.node_id).bind(input.timestamp_ms).bind(old_key).bind(new_key.as_slice())
    .execute(&state.db).await
    .map_err(|error| {
        if error.as_database_error().is_some_and(|error| error.is_unique_violation()) {
            rejected()
        } else {
            auth::unavailable()
        }
    })?;
    if result.rows_affected() != 1 {
        return Err(rejected());
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Serialize, sqlx::FromRow)]
struct Node {
    id: Uuid,
    label: String,
    online: bool,
    revoked: bool,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<impl IntoResponse, AppError> {
    let nodes = sqlx::query_as::<_, Node>(
        "SELECT id,label,COALESCE(revoked_at IS NULL AND last_seen>now()-interval '90 seconds',false) AS online,
         revoked_at IS NOT NULL AS revoked FROM source_nodes WHERE owner_id=$1 AND public_key IS NOT NULL
         ORDER BY created_at DESC LIMIT 100",
    )
    .bind(identity.user.id).fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    Ok((PRIVATE, Json(nodes)))
}

pub async fn revoke(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
) -> Result<StatusCode, AppError> {
    let owner = identity.user.id;
    let result = sqlx::query(
        "WITH revoked AS (UPDATE source_nodes SET revoked_at=now()
         WHERE id=$1 AND owner_id=$2 AND revoked_at IS NULL RETURNING id)
         INSERT INTO source_node_events(node_id,actor_id,event)
         SELECT id,$2,'revoked' FROM revoked",
    )
    .bind(id)
    .bind(owner)
    .execute(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    if result.rows_affected() != 1 {
        return Err(AppError(
            StatusCode::NOT_FOUND,
            "Active node not found.".into(),
        ));
    }
    Ok(StatusCode::NO_CONTENT)
}
