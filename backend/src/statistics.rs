//! Aggregates the authenticated user's recorded progress, never inferred watch time.
use crate::{auth, catalog, AppError, AppState, MediaFile};
use axum::{extract::State, Extension, Json};
use serde::Serialize;
use std::collections::{HashMap, HashSet};
#[derive(sqlx::FromRow)]
struct Row {
    file_key: String,
    source_id: String,
    relative_path: String,
    position: f64,
    total: Option<f64>,
    watched_seconds: Option<f64>,
    units: i32,
    completed: bool,
}
#[derive(Serialize)]
pub struct Genre {
    name: String,
    value: f64,
}
#[derive(Serialize)]
pub struct Statistics {
    watch_seconds: Option<f64>,
    episodes_watched: usize,
    reading_units: usize,
    reading_active: usize,
    books_completed: usize,
    series_completed: Option<usize>,
    watching_genres: Vec<Genre>,
    reading_genres: Vec<Genre>,
    truncated: bool,
}
fn rank(map: HashMap<String, f64>) -> Vec<Genre> {
    let mut list: Vec<_> = map
        .into_iter()
        .filter(|(_, value)| *value > 0.0)
        .map(|(name, value)| Genre { name, value })
        .collect();
    list.sort_by(|a, b| b.value.total_cmp(&a.value).then(a.name.cmp(&b.name)));
    list.truncate(5);
    list
}
pub async fn read(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<Json<Statistics>, AppError> {
    let mut rows=sqlx::query_as::<_,Row>("SELECT p.file_key,p.source_id,p.relative_path,p.position,p.total,p.watched_seconds,cardinality(p.completed_units) AS units,p.completed FROM media_progress p JOIN sources s ON s.id=p.source_id WHERE p.user_id=$1 AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions a WHERE a.source_id=s.id AND a.user_id=$1 AND a.can_discover AND a.can_access_media)) ORDER BY p.updated_at DESC,p.file_key LIMIT 5001").bind(identity.user.id).fetch_all(&state.db).await.map_err(|_|auth::unavailable())?;
    let truncated = rows.len() > 5000;
    rows.truncate(5000);
    let mut items: Vec<catalog::Item> = rows
        .iter()
        .filter_map(|row| {
            let path = std::path::Path::new(&row.relative_path);
            let extension = path.extension()?.to_str()?.to_lowercase();
            let kind = crate::classify_extension(&extension)?;
            Some(catalog::Item {
                file_key: row.file_key.clone(),
                source_id: row.source_id.clone(),
                source_name: String::new(),
                file: MediaFile {
                    name: path.file_name()?.to_string_lossy().into(),
                    relative_path: row.relative_path.clone(),
                    kind,
                    extension,
                    size_bytes: 0,
                    modified_at: None,
                },
                saved_at: None,
                last_opened_at: None,
                available: false,
                metadata: None,
                progress: None,
            })
        })
        .collect();
    crate::metadata::enrich(&state, identity.user.id, &mut items).await?;
    let by_key: HashMap<_, _> = items
        .iter()
        .map(|item| (item.file_key.as_str(), item))
        .collect();
    let mut stats = Statistics {
        watch_seconds: None,
        episodes_watched: 0,
        reading_units: 0,
        reading_active: 0,
        books_completed: 0,
        series_completed: None,
        watching_genres: vec![],
        reading_genres: vec![],
        truncated,
    };
    let (mut watching, mut reading) = (HashMap::new(), HashMap::new());
    let mut active = HashSet::new();
    for row in rows.iter() {
        let Some(item) = by_key.get(row.file_key.as_str()) else {
            continue;
        };
        let fields = item.metadata.as_ref().map(|metadata| &metadata.fields);
        if item.file.kind == "video" {
            if let Some(seconds) = row.watched_seconds {
                *stats.watch_seconds.get_or_insert(0.0) += seconds;
                for genre in fields.and_then(|f| f.genres.as_ref()).into_iter().flatten() {
                    *watching.entry(genre.clone()).or_insert(0.0) += seconds;
                }
            }
            if (row.completed || row.total.is_some_and(|total| row.position >= total))
                && fields.is_some_and(|f| f.episode.is_some())
            {
                stats.episodes_watched += 1;
            }
        } else if item.file.kind == "book" {
            stats.reading_units += row.units as usize;
            if row.completed {
                stats.books_completed += 1;
            } else {
                active.insert(
                    item.metadata
                        .as_ref()
                        .and_then(|m| m.group.as_ref())
                        .map(|group| group.id.clone())
                        .unwrap_or_else(|| row.file_key.clone()),
                );
            }
            for genre in fields.and_then(|f| f.genres.as_ref()).into_iter().flatten() {
                *reading.entry(genre.clone()).or_insert(0.0) += row.units as f64;
            }
        }
    }
    stats.reading_active = active.len();
    stats.watching_genres = rank(watching);
    stats.reading_genres = rank(reading);
    Ok(Json(stats))
}
