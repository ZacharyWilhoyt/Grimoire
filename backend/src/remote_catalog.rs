//! An authenticated, revisioned projection of the agent's locally approved index.
use crate::{auth, classify_extension, nodes, security::ClientIp, AppError, AppState};
use axum::{
    body::Bytes,
    extract::{Path, Query, State},
    http::{header, HeaderMap, StatusCode},
    response::IntoResponse,
    Extension, Json,
};
use grimoire_api::node_protocol::{self, CatalogDelta};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use uuid::Uuid;

const MAX_SOURCE_ITEMS: i64 = 10_000;
const MAX_NODE_SOURCES: i64 = 32;
const PAGE_SIZE: i64 = 100;

fn invalid() -> AppError {
    crate::bad_request("Invalid remote catalog delta.")
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::NOT_FOUND,
        "Remote catalog is unavailable.".into(),
    )
}

fn revision_conflict() -> AppError {
    AppError(
        StatusCode::CONFLICT,
        "Catalog revision is out of order.".into(),
    )
}

fn source_key(id: Uuid) -> String {
    format!("remote:{id}")
}

fn text(value: &str, max: usize) -> bool {
    (1..=max).contains(&value.chars().count())
        && value.trim() == value
        && !value.chars().any(char::is_control)
}

fn validate(delta: &CatalogDelta) -> Result<(), AppError> {
    let mut items = HashSet::new();
    let mut representations = HashSet::new();
    let mut users = HashSet::new();
    if delta.source_id.is_nil()
        || !text(&delta.source_name, 120)
        || delta.revision <= 0
        || delta.upserts.len() + delta.removed.len() > node_protocol::CATALOG_BATCH_LIMIT
        || delta.upserts.iter().any(|entry| {
            entry.item_id.is_nil()
                || !items.insert(entry.item_id)
                || entry.representation_id.is_nil()
                || !representations.insert(entry.representation_id)
                || !text(&entry.name, 255)
                || entry.name.contains(['/', '\\'])
                || classify_extension(&entry.extension).is_none()
                || entry.size_bytes <= 0
        })
        || delta
            .removed
            .iter()
            .any(|id| id.is_nil() || !items.insert(*id))
        || delta.grants.as_ref().is_some_and(|grants| {
            grants.len() > 100
                || grants.iter().any(|grant| {
                    grant.user_id.is_nil()
                        || !users.insert(grant.user_id)
                        || grant.can_access_media && !grant.can_discover
                })
        })
    {
        return Err(invalid());
    }
    Ok(())
}

pub async fn delta(
    State(state): State<AppState>,
    Extension(ClientIp(ip)): Extension<ClientIp>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<StatusCode, AppError> {
    let field = |key| {
        headers
            .get(key)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(nodes::rejected)
    };
    let version = field("x-grimoire-node-version")?
        .parse()
        .map_err(|_| nodes::rejected())?;
    nodes::version(version)?;
    let node_id: Uuid = field("x-grimoire-node-id")?
        .parse()
        .map_err(|_| nodes::rejected())?;
    let timestamp: i64 = field("x-grimoire-node-timestamp")?
        .parse()
        .map_err(|_| nodes::rejected())?;
    if timestamp.abs_diff(node_protocol::now_ms()) > node_protocol::CLOCK_SKEW_MS {
        return Err(nodes::rejected());
    }
    auth::rate_limit(&state.db, format!("nodes:catalog:{ip}"), 120, 60).await?;
    let digest = node_protocol::body_digest(&body);
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    // Serialize deltas, revocation and key rotation for this identity.
    let (owner, key): (Uuid, Vec<u8>) = sqlx::query_as(
        "SELECT n.owner_id,n.public_key FROM source_nodes n JOIN users u ON u.id=n.owner_id
         WHERE n.id=$1 AND n.revoked_at IS NULL AND n.public_key IS NOT NULL AND NOT u.disabled
         FOR UPDATE OF n",
    )
    .bind(node_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(nodes::rejected)?;
    if !node_protocol::verify(
        &key,
        &node_protocol::catalog_signature_bytes(version, node_id, timestamp, &digest),
        field("x-grimoire-node-signature")?,
    ) {
        return Err(nodes::rejected());
    }
    let delta: CatalogDelta = serde_json::from_slice(&body).map_err(|_| invalid())?;
    validate(&delta)?;
    let source = source_key(delta.source_id);
    let current: Option<(i64, String)> = sqlx::query_as(
        "SELECT c.revision,c.digest FROM sources s JOIN remote_catalog_state c ON c.source_id=s.id
         WHERE s.id=$1 AND s.node_id=$2 AND s.owner_id=$3",
    )
    .bind(&source)
    .bind(node_id)
    .bind(owner)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if let Some((revision, previous)) = current {
        if delta.revision == revision && digest == previous {
            return Ok(StatusCode::NO_CONTENT);
        }
        if revision.checked_add(1) != Some(delta.revision) {
            return Err(revision_conflict());
        }
        sqlx::query("UPDATE sources SET name=$2 WHERE id=$1 AND node_id=$3")
            .bind(&source)
            .bind(&delta.source_name)
            .bind(node_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
        sqlx::query(
            "UPDATE remote_catalog_state SET revision=$2,digest=$3,published=$4,last_confirmed=now() WHERE source_id=$1",
        )
        .bind(&source)
        .bind(delta.revision)
        .bind(&digest)
        .bind(delta.published)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    } else {
        if delta.revision != 1 {
            return Err(revision_conflict());
        }
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM sources WHERE node_id=$1")
            .bind(node_id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
        if count >= MAX_NODE_SOURCES {
            return Err(invalid());
        }
        let inserted = sqlx::query(
            "INSERT INTO sources(id,owner_id,name,kind,node_id) VALUES($1,$2,$3,'remote_agent',$4)
             ON CONFLICT DO NOTHING",
        )
        .bind(&source)
        .bind(owner)
        .bind(&delta.source_name)
        .bind(node_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?
        .rows_affected();
        if inserted != 1 {
            return Err(unavailable());
        }
        sqlx::query("INSERT INTO remote_catalog_state(source_id,revision,digest,published) VALUES($1,$2,$3,$4)")
            .bind(&source).bind(delta.revision).bind(&digest).bind(delta.published)
            .execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
    }

    sqlx::query("DELETE FROM remote_media_items WHERE source_id=$1 AND id=ANY($2)")
        .bind(&source)
        .bind(&delta.removed)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    for entry in &delta.upserts {
        sqlx::query(
            "INSERT INTO remote_media_items(source_id,id,name) VALUES($1,$2,$3)
                     ON CONFLICT(source_id,id) DO UPDATE SET name=EXCLUDED.name",
        )
        .bind(&source)
        .bind(entry.item_id)
        .bind(&entry.name)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
        let changed = sqlx::query(
            "INSERT INTO remote_media_representations(source_id,item_id,id,extension,size_bytes)
             VALUES($1,$2,$3,$4,$5) ON CONFLICT(source_id,id) DO UPDATE
                SET extension=EXCLUDED.extension,size_bytes=EXCLUDED.size_bytes
                WHERE remote_media_representations.item_id=EXCLUDED.item_id",
        )
        .bind(&source)
        .bind(entry.item_id)
        .bind(entry.representation_id)
        .bind(&entry.extension)
        .bind(entry.size_bytes)
        .execute(&mut *tx)
        .await
        .map_err(|error| {
            if error
                .as_database_error()
                .is_some_and(|error| error.is_unique_violation())
            {
                invalid()
            } else {
                auth::unavailable()
            }
        })?
        .rows_affected();
        if changed != 1 {
            return Err(invalid());
        }
        sqlx::query(
            "INSERT INTO remote_media_locations(source_id,representation_id,node_id)
                     VALUES($1,$2,$3) ON CONFLICT DO NOTHING",
        )
        .bind(&source)
        .bind(entry.representation_id)
        .bind(node_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    }
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM remote_media_items WHERE source_id=$1")
            .bind(&source)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    if count > MAX_SOURCE_ITEMS {
        return Err(invalid());
    }
    if let Some(grants) = &delta.grants {
        sqlx::query("DELETE FROM remote_source_grants WHERE source_id=$1")
            .bind(&source)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
        for grant in grants {
            sqlx::query(
                "INSERT INTO remote_source_grants(source_id,user_id,can_discover,can_access_media)
                         SELECT $1,id,$3,$4 FROM users WHERE id=$2 AND NOT disabled",
            )
            .bind(&source)
            .bind(grant.user_id)
            .bind(grant.can_discover)
            .bind(grant.can_access_media)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
        }
    }
    let changed: Vec<_> = delta
        .upserts
        .iter()
        .map(|entry| entry.representation_id)
        .collect();
    crate::catalog_inventory::project_remote(&mut tx, &source, &changed)
        .await
        .map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct Cursor {
    after: Option<Uuid>,
}

#[derive(Serialize, sqlx::FromRow)]
struct Entry {
    id: Uuid,
    representation_id: Uuid,
    name: String,
    extension: String,
    size_bytes: i64,
}

#[derive(Serialize)]
struct CatalogPage {
    items: Vec<Entry>,
    next: Option<Uuid>,
    can_access_media: bool,
}

pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source_id): Path<Uuid>,
    Query(cursor): Query<Cursor>,
) -> Result<impl IntoResponse, AppError> {
    let source = source_key(source_id);
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ READ ONLY")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let can_access_media: bool = sqlx::query_scalar(
        "SELECT s.owner_id=$2 OR COALESCE(p.can_access_media AND g.can_access_media,false)
         FROM sources s JOIN source_nodes n ON n.id=s.node_id
         JOIN users u ON u.id=s.owner_id AND NOT u.disabled
         JOIN remote_catalog_state c ON c.source_id=s.id
         LEFT JOIN source_permissions p ON p.source_id=s.id AND p.user_id=$2
         LEFT JOIN remote_source_grants g ON g.source_id=s.id AND g.user_id=$2
         WHERE s.id=$1 AND c.published AND c.last_confirmed>now()-interval '90 seconds'
           AND n.revoked_at IS NULL AND n.last_seen>now()-interval '90 seconds'
           AND (s.owner_id=$2 OR (p.can_discover AND g.can_discover))",
    )
    .bind(&source)
    .bind(identity.user.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(unavailable)?;
    let mut items = sqlx::query_as::<_, Entry>(
        "SELECT i.id,r.id AS representation_id,i.name,r.extension,r.size_bytes
         FROM remote_media_items i JOIN remote_media_representations r
           ON r.source_id=i.source_id AND r.item_id=i.id
         WHERE i.source_id=$1 AND i.id>$2 ORDER BY i.id LIMIT $3",
    )
    .bind(&source)
    .bind(cursor.after.unwrap_or(Uuid::nil()))
    .bind(PAGE_SIZE + 1)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    let next = if items.len() as i64 > PAGE_SIZE {
        items.pop();
        items.last().map(|item| item.id)
    } else {
        None
    };
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(CatalogPage {
            items,
            next,
            can_access_media,
        }),
    ))
}

#[derive(Serialize, sqlx::FromRow)]
struct SourceSummary {
    id: String,
    name: String,
    is_owner: bool,
    node_id: Option<Uuid>,
    available: bool,
}

pub async fn sources(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<impl IntoResponse, AppError> {
    let rows = sqlx::query_as::<_, SourceSummary>(
        "SELECT substring(s.id FROM 8) AS id,s.name,s.owner_id=$1 AS is_owner,
         CASE WHEN s.owner_id=$1 THEN n.id END AS node_id,
         COALESCE(c.published AND n.revoked_at IS NULL AND n.last_seen>now()-interval '90 seconds'
            AND c.last_confirmed>now()-interval '90 seconds',false) AS available
         FROM sources s JOIN source_nodes n ON n.id=s.node_id
         JOIN users u ON u.id=s.owner_id AND NOT u.disabled
         JOIN remote_catalog_state c ON c.source_id=s.id
         LEFT JOIN source_permissions p ON p.source_id=s.id AND p.user_id=$1
         LEFT JOIN remote_source_grants g ON g.source_id=s.id AND g.user_id=$1
         WHERE s.kind='remote_agent' AND (s.owner_id=$1 OR
             (p.can_discover AND g.can_discover AND c.published AND n.revoked_at IS NULL
              AND n.last_seen>now()-interval '90 seconds' AND c.last_confirmed>now()-interval '90 seconds'))
         ORDER BY s.name,s.id LIMIT 100",
    )
    .bind(identity.user.id).fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    Ok(([(header::CACHE_CONTROL, "private, no-store")], Json(rows)))
}
