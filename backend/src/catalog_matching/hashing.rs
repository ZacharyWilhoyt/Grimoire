use super::{missing, private};
use crate::{auth, AppError, AppState};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::Response,
    Extension,
};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use std::{io, path::PathBuf, time::Duration};
use tokio::{io::AsyncReadExt, sync::Semaphore};
use uuid::Uuid;

const CHUNK: usize = 1024 * 1024;
const MAX_BYTES: i64 = 64 * 1024 * 1024 * 1024;
const TIMEOUT: Duration = Duration::from_secs(15 * 60);
static HASH_WORKER: Semaphore = Semaphore::const_new(1);

#[derive(FromRow)]
struct Input {
    representation_id: Uuid,
    root: String,
    relative: String,
    kind: String,
    size: i64,
    modified_at: Option<f64>,
    digest: Option<String>,
    job_state: Option<String>,
}

async fn input(db: &PgPool, user: Uuid, id: Uuid) -> Result<Input, AppError> {
    sqlx::query_as(
        "SELECT l.representation_id,s.path AS root,l.local_relative_path AS relative,
                i.kind,r.size_bytes AS size,l.local_modified_at AS modified_at,
                r.content_sha256 AS digest,
                CASE WHEN j.state='running' AND j.started_at<=now()-interval '16 minutes'
                     THEN 'failed' ELSE j.state END AS job_state
         FROM catalog_locations l JOIN sources s ON s.id=l.source_id
         JOIN users u ON u.id=s.owner_id AND NOT u.disabled
         JOIN catalog_representations r ON r.id=l.representation_id
         JOIN catalog_items i ON i.id=r.item_id
         LEFT JOIN catalog_hash_jobs j ON j.representation_id=r.id
         WHERE l.id=$1 AND s.owner_id=$2 AND l.local_file_key IS NOT NULL",
    )
    .bind(id)
    .bind(user)
    .fetch_optional(db)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(missing)
}

pub(super) async fn status(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let file = input(&state.db, identity.user.id, id).await?;
    let status = if file.digest.is_some() {
        "ready"
    } else {
        file.job_state.as_deref().unwrap_or("idle")
    };
    Ok(private(json!({"state":status,"sha256":file.digest})))
}

pub(super) async fn start(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let user = identity.user.id;
    let file = input(&state.db, user, id).await?;
    if let Some(digest) = file.digest {
        return Ok(private(json!({"state":"ready","sha256":digest})));
    }
    if file.job_state.as_deref() == Some("running") {
        return Ok(private(json!({"state":"running"})));
    }
    if file.modified_at.is_none() || file.size > MAX_BYTES {
        return Err(crate::bad_request(
            "Hashing needs a current local observation and a file up to 64 GiB.",
        ));
    }
    auth::rate_limit(&state.db, format!("catalog:hash:{user}"), 4, 3600).await?;
    let permit = HASH_WORKER.try_acquire().map_err(|_| {
        AppError(
            StatusCode::TOO_MANY_REQUESTS,
            "The catalog hash worker is busy.".into(),
        )
    })?;
    let job = Uuid::new_v4();
    let claimed = sqlx::query(
        "INSERT INTO catalog_hash_jobs(representation_id,id,state) VALUES($1,$2,'running')
         ON CONFLICT(representation_id) DO UPDATE SET id=$2,state='running',started_at=now()
         WHERE catalog_hash_jobs.state='failed'
           OR (catalog_hash_jobs.state='running' AND catalog_hash_jobs.started_at<=now()-interval '16 minutes')",
    )
    .bind(file.representation_id).bind(job).execute(&state.db).await.map_err(|_| auth::unavailable())?;
    if claimed.rows_affected() != 0 {
        let db = state.db.clone();
        tokio::spawn(async move {
            let _permit = permit;
            let digest = tokio::time::timeout(TIMEOUT, read(&file))
                .await
                .ok()
                .and_then(Result::ok);
            let _ = finish(&db, id, user, job, file.representation_id, digest).await;
        });
    }
    Ok(private(json!({"state":"running"})))
}

async fn read(input: &Input) -> io::Result<String> {
    let root = PathBuf::from(&input.root);
    let relative = input.relative.clone();
    let kind = input.kind.clone();
    let unavailable = || io::Error::other("File changed or became unavailable");
    let path =
        tokio::task::spawn_blocking(move || crate::resolve_media_path(&root, &relative, &kind))
            .await
            .map_err(|_| unavailable())?
            .map_err(|_| unavailable())?;
    let valid = |stat: &std::fs::Metadata| {
        stat.is_file()
            && stat.len() == input.size as u64
            && crate::modified_epoch(stat) == input.modified_at
    };
    let mut file = tokio::fs::File::open(&path).await?;
    if !valid(&file.metadata().await?) {
        return Err(unavailable());
    }
    let mut remaining = input.size as u64;
    let mut buffer = vec![0; CHUNK];
    let mut hash = Sha256::new();
    while remaining > 0 {
        let length = remaining.min(CHUNK as u64) as usize;
        file.read_exact(&mut buffer[..length]).await?;
        hash.update(&buffer[..length]);
        remaining -= length as u64;
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    if !valid(&file.metadata().await?) || !valid(&tokio::fs::metadata(path).await?) {
        return Err(unavailable());
    }
    Ok(hex::encode(hash.finalize()))
}

async fn finish(
    db: &PgPool,
    location: Uuid,
    owner: Uuid,
    job: Uuid,
    representation: Uuid,
    digest: Option<String>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "WITH stored AS (
             UPDATE catalog_representations r SET content_sha256=$5
             WHERE r.id=$4 AND $5::text IS NOT NULL AND EXISTS(
                 SELECT 1 FROM catalog_locations l JOIN sources s ON s.id=l.source_id
                 JOIN users u ON u.id=s.owner_id AND NOT u.disabled
                 JOIN catalog_hash_jobs j ON j.representation_id=l.representation_id AND j.id=$3
                 WHERE l.id=$1 AND u.id=$2 AND l.representation_id=r.id)
             RETURNING r.id
         )
         UPDATE catalog_hash_jobs SET state=CASE WHEN EXISTS(SELECT 1 FROM stored)
             THEN 'ready' ELSE 'failed' END WHERE representation_id=$4 AND id=$3",
    )
    .bind(location)
    .bind(owner)
    .bind(job)
    .bind(representation)
    .bind(digest)
    .execute(db)
    .await?;
    Ok(())
}
