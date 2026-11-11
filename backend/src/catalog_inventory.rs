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
                 file_key text, source_id text, path text, kind text, extension text, size bigint)
             JOIN sources s ON s.id=o.source_id AND s.kind='local_directory'
         ), incoming AS MATERIALIZED (
             SELECT o.*, gen_random_uuid() AS location_id, gen_random_uuid() AS representation_id,
                    gen_random_uuid() AS item_id
             FROM observed o LEFT JOIN catalog_locations l ON l.local_file_key=o.file_key
             WHERE l.id IS NULL
         ), created_items AS (
             INSERT INTO catalog_items(id,kind) SELECT item_id,kind FROM incoming RETURNING id
         ), created_representations AS (
             INSERT INTO catalog_representations(id,item_id,extension,size_bytes)
             SELECT i.representation_id,c.id,i.extension,i.size
             FROM incoming i JOIN created_items c ON c.id=i.item_id RETURNING id
         ), created_locations AS (
             INSERT INTO catalog_locations(id,representation_id,source_id,local_file_key,local_relative_path)
             SELECT i.location_id,r.id,i.source_id,i.file_key,i.path
             FROM incoming i JOIN created_representations r ON r.id=i.representation_id
         )
         UPDATE catalog_locations l SET last_observed_at=now()
         FROM observed o WHERE l.local_file_key=o.file_key
           AND l.last_observed_at < now()-interval '1 minute'",
    )
    .bind(batch)
    .execute(&mut *tx)
    .await?;
    tx.commit().await
}
