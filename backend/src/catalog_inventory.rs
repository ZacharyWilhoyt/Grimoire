//! Record local inventory observations without changing the legacy catalog response.
use crate::{catalog::Item, AppState};
use serde::Serialize;
use sqlx::PgPool;

const BATCH_SIZE: usize = 256;
const LOCK: i64 = 0x4752_494d_494e_5631;

#[derive(Serialize)]
struct Observation<'a> {
    file_key: &'a str,
    source_id: &'a str,
    path: &'a str,
    kind: &'a str,
    extension: &'a str,
    size: i64,
    modified_at: Option<f64>,
}

pub(crate) fn observe(state: &AppState, items: &[Item]) {
    if items.is_empty() {
        return;
    }
    let Ok(permit) = state.inventory_writes.clone().try_acquire_owned() else {
        return;
    };
    let batches: Vec<_> = items
        .chunks(BATCH_SIZE)
        .map(|chunk| {
            serde_json::json!(chunk
                .iter()
                .filter_map(|item| {
                    Some(Observation {
                        file_key: &item.file_key,
                        source_id: &item.source_id,
                        path: &item.file.relative_path,
                        kind: item.file.kind,
                        extension: &item.file.extension,
                        size: item.file.size_bytes.try_into().ok()?,
                        modified_at: item.file.modified_at,
                    })
                })
                .collect::<Vec<_>>())
        })
        .collect();
    let db = state.db.clone();
    tokio::spawn(async move {
        let _permit = permit;
        for batch in batches {
            if !matches!(
                tokio::time::timeout(std::time::Duration::from_secs(2), record(&db, batch)).await,
                Ok(Ok(()))
            ) {
                break;
            }
        }
    });
}

pub(crate) async fn record(db: &PgPool, batch: serde_json::Value) -> Result<(), sqlx::Error> {
    let mut tx = db.begin().await?;
    // Serialize first observations across coordinator processes. A busy writer
    // can be retried on a later scan.
    if !sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
        .bind(LOCK)
        .fetch_one(&mut *tx)
        .await?
    {
        return Ok(());
    }
    sqlx::query(
        "WITH observed AS MATERIALIZED (
             SELECT o.* FROM jsonb_to_recordset($1) AS o(
                 file_key text, source_id text, path text, kind text, extension text, size bigint,
                 modified_at double precision)
             JOIN sources s ON s.id=o.source_id AND s.kind='local_directory'
         ), incoming AS MATERIALIZED (
             SELECT o.*, gen_random_uuid() AS location_id, gen_random_uuid() AS representation_id,
                    gen_random_uuid() AS item_id
             FROM observed o LEFT JOIN catalog_locations l ON l.local_file_key=o.file_key
             LEFT JOIN catalog_representations r ON r.id=l.representation_id
             WHERE r.id IS NULL OR r.extension<>o.extension OR r.size_bytes<>o.size
                OR (l.local_modified_at IS NOT NULL AND o.modified_at IS NOT NULL
                    AND l.local_modified_at<>o.modified_at)
         ), created_items AS (
             INSERT INTO catalog_items(id,kind) SELECT item_id,kind FROM incoming RETURNING id
         ), created_representations AS (
             INSERT INTO catalog_representations(id,item_id,extension,size_bytes)
             SELECT i.representation_id,c.id,i.extension,i.size
             FROM incoming i JOIN created_items c ON c.id=i.item_id RETURNING id
         ), created_locations AS (
             INSERT INTO catalog_locations(id,representation_id,source_id,local_file_key,local_relative_path,local_modified_at)
             SELECT i.location_id,r.id,i.source_id,i.file_key,i.path,i.modified_at
             FROM incoming i JOIN created_representations r ON r.id=i.representation_id
             ON CONFLICT(local_file_key) DO UPDATE
                 SET representation_id=EXCLUDED.representation_id,last_observed_at=now(),
                     local_modified_at=EXCLUDED.local_modified_at
             RETURNING local_file_key
         )
         UPDATE catalog_locations l SET last_observed_at=now(),
                                        local_modified_at=COALESCE(o.modified_at,l.local_modified_at)
         FROM observed o WHERE l.local_file_key=o.file_key
           AND (l.last_observed_at < now()-interval '1 minute'
                OR (l.local_modified_at IS NULL AND o.modified_at IS NOT NULL))
           AND NOT EXISTS(SELECT 1 FROM created_locations c WHERE c.local_file_key=l.local_file_key)",
    )
    .bind(batch)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}

/// Seed newly accepted representations and a bounded batch of preexisting
/// records. The caller holds the source node lock and commits the signed delta.
pub(crate) async fn project_remote(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    source: &str,
    changed: &[uuid::Uuid],
) -> Result<(), sqlx::Error> {
    let rows: Vec<(uuid::Uuid, String, i64)> = sqlx::query_as(
        "SELECT id,extension,size_bytes FROM remote_media_representations
         WHERE source_id=$1 AND id=ANY($2)
         UNION ALL
         (SELECT r.id,r.extension,r.size_bytes FROM remote_media_representations r
          WHERE r.source_id=$1 AND NOT (r.id=ANY($2)) AND NOT EXISTS (
              SELECT 1 FROM catalog_locations l
              WHERE l.source_id=r.source_id AND l.remote_representation_id=r.id)
          ORDER BY r.id LIMIT $3)",
    )
    .bind(source)
    .bind(changed)
    .bind(BATCH_SIZE as i64)
    .fetch_all(&mut **tx)
    .await?;
    if rows.is_empty() {
        return Ok(());
    }

    let batch: Vec<_> = rows
        .iter()
        .filter_map(|(id, extension, size)| {
            Some(serde_json::json!({
                "id":id,"extension":extension,"size":size,
                "kind":crate::classify_extension(extension)?,
            }))
        })
        .collect();
    sqlx::query(
        "WITH incoming AS MATERIALIZED (
             SELECT o.*,gen_random_uuid() AS item_id,gen_random_uuid() AS representation_id
             FROM jsonb_to_recordset($2) AS o(id uuid,extension text,size bigint,kind text)
             LEFT JOIN catalog_locations l ON l.source_id=$1 AND l.remote_representation_id=o.id
             LEFT JOIN catalog_representations r ON r.id=l.representation_id
             WHERE r.id IS NULL OR r.extension<>o.extension OR r.size_bytes<>o.size
         ), created_items AS (
             INSERT INTO catalog_items(id,kind) SELECT item_id,kind FROM incoming RETURNING id
         ), created_representations AS (
             INSERT INTO catalog_representations(id,item_id,extension,size_bytes)
             SELECT i.representation_id,c.id,i.extension,i.size
             FROM incoming i JOIN created_items c ON c.id=i.item_id RETURNING id
         )
         INSERT INTO catalog_locations(id,representation_id,source_id,remote_representation_id)
         SELECT gen_random_uuid(),r.id,$1,i.id
         FROM incoming i JOIN created_representations r ON r.id=i.representation_id
         ON CONFLICT(source_id,remote_representation_id) DO UPDATE
             SET representation_id=EXCLUDED.representation_id,last_observed_at=now()",
    )
    .bind(source)
    .bind(serde_json::Value::Array(batch))
    .execute(&mut **tx)
    .await?;
    Ok(())
}
