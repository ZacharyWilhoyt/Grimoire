//! Reconciliation changes catalog projections without rewriting source records.
mod hashing;
use crate::{auth, bad_request, metadata, AppError, AppState};
use axum::{
    extract::{DefaultBodyLimit, Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Extension, Json, Router,
};
use metadata::{model::Fields, providers::Provider};
use serde::{Deserialize, Serialize};
use sqlx::{FromRow, PgConnection, Postgres, Transaction};
use std::collections::HashSet;
use uuid::Uuid;

const LOCK: i64 = 0x4752_494d_4d41_5443;
const MAX_BATCH: usize = 50;

pub(crate) fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/catalog/v1/items", get(items))
        .route("/api/catalog/v1/items/{id}", get(item))
        .route(
            "/api/catalog/v1/locations/{location}/suggestions",
            get(suggestions),
        )
        .route("/api/catalog/v1/review/preview", post(preview))
        .route("/api/catalog/v1/review", post(apply))
        .route("/api/catalog/v1/review/{batch}/undo", post(undo))
        .route("/api/admin/catalog/v1/verify", post(verify))
        .route("/api/catalog/v1/locations/{location}/report", post(report))
        .route("/api/admin/catalog/v1/cases", get(cases))
        .route(
            "/api/admin/catalog/v1/cases/{id}/resolve",
            post(resolve_case),
        )
        .route(
            "/api/admin/catalog/v1/sources/{source}/suppression",
            post(suppress),
        )
        .route(
            "/api/catalog/v1/sources/{source}/review-candidates",
            get(batch_candidates),
        )
        .route(
            "/api/catalog/v1/locations/{location}/hash",
            get(hashing::status).post(hashing::start),
        )
        .layer(DefaultBodyLimit::max(16 * 1024))
}

fn missing() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Catalog entry not found.".into())
}

fn conflict() -> AppError {
    AppError(
        StatusCode::CONFLICT,
        "The catalog entry changed. Refresh the preview.".into(),
    )
}

fn private<T: Serialize>(data: T) -> Response {
    ([("cache-control", "private, no-store")], Json(data)).into_response()
}

#[derive(Deserialize, Default)]
struct Page {
    after: Option<Uuid>,
    limit: Option<u16>,
}

#[derive(Serialize, FromRow)]
struct CatalogItem {
    id: Uuid,
    location_id: Uuid,
    representation_id: Uuid,
    kind: String,
    name: String,
    playback_eligible: bool,
}

async fn items(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(page): Query<Page>,
) -> Result<Response, AppError> {
    let limit = page.limit.unwrap_or(30).clamp(1, 100) as usize;
    let mut rows = sqlx::query_as::<_, CatalogItem>(
        "SELECT DISTINCT ON (v.logical_id) v.logical_id AS id, v.location_id,
                v.representation_id, i.kind, v.name, v.playback_eligible
         FROM catalog_matched_locations v JOIN catalog_items i ON i.id=v.item_id
         WHERE v.viewer_id=$1 AND ($2::uuid IS NULL OR v.logical_id>$2)
         ORDER BY v.logical_id,v.playback_eligible DESC,v.online DESC,v.location_id
         LIMIT $3",
    )
    .bind(identity.user.id)
    .bind(page.after)
    .bind((limit + 1) as i64)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next = more.then(|| rows.last().map(|row| row.id)).flatten();
    Ok(private(serde_json::json!({"items":rows,"next":next})))
}

async fn item(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
) -> Result<Response, AppError> {
    let row = sqlx::query_as::<_, CatalogItem>(
        "WITH resolved AS (
             SELECT logical_id FROM catalog_matched_locations
             WHERE viewer_id=$1 AND (item_id=$2 OR logical_id=$2 OR EXISTS (
                 SELECT 1 FROM catalog_review_events e
                 WHERE e.location_id=catalog_matched_locations.location_id
                   AND e.representation_id=catalog_matched_locations.representation_id
                   AND e.verified_identity_id=$2
             ))
             ORDER BY (logical_id=$2) DESC,(item_id=$2) DESC,logical_id LIMIT 1
         )
         SELECT v.logical_id AS id,v.location_id,v.representation_id,i.kind,v.name,v.playback_eligible
         FROM resolved r JOIN catalog_matched_locations v ON v.logical_id=r.logical_id
         JOIN catalog_items i ON i.id=v.item_id
         WHERE v.viewer_id=$1 ORDER BY v.playback_eligible DESC,v.online DESC,v.location_id LIMIT 1",
    )
    .bind(identity.user.id).bind(id)
    .fetch_optional(&state.db).await.map_err(|_| auth::unavailable())?.ok_or_else(missing)?;
    Ok(private(row))
}

#[derive(Serialize, FromRow)]
struct Location {
    location_id: Uuid,
    representation_id: Uuid,
    kind: String,
    name: String,
    revision: Option<i64>,
    #[serde(skip)]
    canonical_id: Option<Uuid>,
    #[serde(skip)]
    entity_type: Option<String>,
    #[serde(skip)]
    suppress_verified: bool,
    #[serde(skip)]
    scope: Option<String>,
    #[serde(skip)]
    source_id: String,
    #[serde(skip)]
    active_link: Option<i64>,
}

async fn location(
    db: &mut PgConnection,
    viewer: Uuid,
    id: Uuid,
    require_owner: bool,
) -> Result<Location, AppError> {
    sqlx::query_as(
        "SELECT v.location_id,v.representation_id,i.kind,v.name,s.id AS source_id,
                c.id AS revision,c.scope,c.active_link,
                COALESCE(c.suppress_verified,false) AS suppress_verified,e.canonical_id,
                COALESCE(e.entity_type,CASE
                    WHEN m.provider IN ('tmdb_movie','arr_radarr') THEN 'movie'
                    WHEN m.provider IN ('tmdb_tv','arr_sonarr') THEN 'episode'
                    WHEN i.kind='video' AND v.name ~* '(^|[^[:alnum:]])s[0-9]{1,4}e[0-9]{1,5}' THEN 'episode'
                    WHEN i.kind='music' THEN 'recording'
                    WHEN i.kind='book' THEN 'work'
                END) AS entity_type
         FROM catalog_discoverable_locations v
         JOIN sources s ON s.id=v.source_id JOIN catalog_items i ON i.id=v.item_id
         JOIN catalog_locations l ON l.id=v.location_id
         LEFT JOIN media_metadata m ON m.file_key=l.local_file_key
         LEFT JOIN catalog_matching_current c
           ON c.location_id=v.location_id AND c.representation_id=v.representation_id
         LEFT JOIN catalog_verified_identities e ON e.representation_id=v.representation_id
         WHERE v.viewer_id=$1 AND v.location_id=$2 AND (NOT $3 OR s.owner_id=$1)",
    )
    .bind(viewer)
    .bind(id)
    .bind(require_owner)
    .fetch_optional(db)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(missing)
}

async fn suggestions(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    Query(page): Query<Page>,
) -> Result<Response, AppError> {
    let actor = identity.user.id;
    auth::rate_limit(&state.db, format!("catalog:suggestions:{actor}"), 60, 300).await?;
    let mut db = state.db.acquire().await.map_err(|_| auth::unavailable())?;
    let own = location(&mut db, actor, id, true).await?;
    let limit = page.limit.unwrap_or(10).clamp(1, 25) as usize;
    let mut rows = sqlx::query_as::<_, CatalogItem>(
        "SELECT DISTINCT ON (v.logical_id) v.logical_id AS id, v.location_id,
                v.representation_id,i.kind,v.name,v.playback_eligible
         FROM catalog_matched_locations v JOIN catalog_items i ON i.id=v.item_id
         LEFT JOIN catalog_verified_identities e ON e.representation_id=v.representation_id
         WHERE v.viewer_id=$1 AND v.location_id<>$2 AND i.kind=$3
           AND ($4::uuid IS NULL OR v.logical_id>$4)
           AND NOT EXISTS (SELECT 1 FROM catalog_review_suppressions suppressed
                           WHERE suppressed.source_id IN ($9,v.source_id))
           AND NOT EXISTS (SELECT 1 FROM catalog_matching_current c
                           WHERE c.location_id=v.location_id
                           AND c.representation_id=v.representation_id AND c.active_link IS NOT NULL)
           AND ((catalog_match_name(v.name)=catalog_match_name($5) AND catalog_match_name($5)<>'')
                OR (v.playback_eligible AND EXISTS (SELECT 1 FROM catalog_representations a JOIN catalog_representations b
                    ON a.content_sha256=b.content_sha256
                    WHERE a.id=$7 AND b.id=v.representation_id)))
           AND ($6::uuid IS NULL OR e.canonical_id IS NULL OR e.canonical_id=$6)
           AND NOT EXISTS (SELECT 1 FROM catalog_matched_locations own
                           WHERE own.viewer_id=$1 AND own.location_id=$2 AND own.logical_id=v.logical_id)
           AND NOT EXISTS (
               SELECT 1 FROM catalog_review_events rejected
               WHERE rejected.location_id=$2 AND rejected.representation_id=$7
                 AND rejected.target_representation_id=v.representation_id AND rejected.action='decline'
                 AND rejected.candidate_signature=md5(jsonb_build_array($5::text,v.name)::text))
         ORDER BY v.logical_id,v.location_id LIMIT $8",
    )
    .bind(actor)
    .bind(id)
    .bind(&own.kind)
    .bind(page.after)
    .bind(&own.name)
    .bind(own.canonical_id)
    .bind(own.representation_id)
    .bind((limit + 1) as i64)
    .bind(&own.source_id)
    .fetch_all(&mut *db)
    .await
    .map_err(|_| auth::unavailable())?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next = more.then(|| rows.last().map(|row| row.id)).flatten();
    Ok(private(
        serde_json::json!({"location":own,"suggestions":rows,"next":next}),
    ))
}

#[derive(Deserialize)]
#[serde(rename_all = "snake_case")]
enum BatchPattern {
    Directory,
    Series,
    Album,
}

#[derive(Deserialize)]
struct BatchQuery {
    pattern: BatchPattern,
    value: String,
    season: Option<u32>,
    after: Option<Uuid>,
}

async fn batch_candidates(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<BatchQuery>,
) -> Result<Response, AppError> {
    let actor = identity.user.id;
    let value = query.value.trim();
    let mode = match query.pattern {
        BatchPattern::Directory => "directory",
        BatchPattern::Series => "video",
        BatchPattern::Album => "music",
    };
    if value.len() > 500
        || (mode != "directory" && value.is_empty())
        || (mode != "video" && query.season.is_some())
    {
        return Err(bad_request(
            "Specify a directory, series or album and an optional series season.",
        ));
    }
    auth::rate_limit(&state.db, format!("catalog:preview:{actor}"), 30, 300).await?;
    let rows = sqlx::query_as::<_, Location>(
        "SELECT v.location_id,v.representation_id,i.kind,v.name,s.id AS source_id,
                c.id AS revision,c.scope,c.active_link,COALESCE(c.suppress_verified,false)
                AS suppress_verified,e.canonical_id,NULL::text AS entity_type
         FROM catalog_discoverable_locations v
         JOIN sources s ON s.id=v.source_id AND s.owner_id=$1
         JOIN catalog_items i ON i.id=v.item_id
         JOIN catalog_locations l ON l.id=v.location_id
         LEFT JOIN media_metadata m ON m.file_key=l.local_file_key
         LEFT JOIN catalog_matching_current c
             ON c.location_id=v.location_id AND c.representation_id=v.representation_id
         LEFT JOIN catalog_verified_identities e ON e.representation_id=v.representation_id
         WHERE v.viewer_id=$1 AND v.source_id=$2 AND ($3::uuid IS NULL OR v.location_id>$3)
           AND CASE WHEN $4='directory' THEN l.local_relative_path IS NOT NULL AND
                    CASE WHEN strpos(l.local_relative_path,'/')=0 THEN ''
                         ELSE regexp_replace(l.local_relative_path,'/[^/]+$','') END=$5
                ELSE i.kind=$4 AND lower(trim(COALESCE(m.custom_data::jsonb->>'collection',
                    m.matched_data::jsonb->>'collection', m.local_data::jsonb->>'collection')))=lower($5)
                    AND ($6::text IS NULL OR COALESCE(m.custom_data::jsonb->>'season',
                         m.matched_data::jsonb->>'season',m.local_data::jsonb->>'season')=$6)
                END
         ORDER BY v.location_id LIMIT 51",
    )
    .bind(actor).bind(source).bind(query.after).bind(mode).bind(value)
    .bind(query.season.map(|season|season.to_string()))
    .fetch_all(&state.db).await.map_err(|_| auth::unavailable())?;
    let next = rows.get(50).map(|_| rows[49].location_id);
    Ok(private(
        serde_json::json!({"locations":&rows[..rows.len().min(50)],"next":next}),
    ))
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Action {
    Link,
    Decline,
    Unlink,
    Split,
}

impl Action {
    fn as_str(self) -> &'static str {
        match self {
            Self::Link => "link",
            Self::Decline => "decline",
            Self::Unlink => "unlink",
            Self::Split => "split",
        }
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Review {
    changes: Vec<Change>,
    reason: String,
    #[serde(default)]
    curator: bool,
}

#[derive(Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct Snapshot {
    representation_id: Uuid,
    revision: Option<i64>,
    target_representation_id: Option<Uuid>,
    identity: Option<Uuid>,
    target_identity: Option<Uuid>,
    target_revision: Option<i64>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Change {
    location_id: Uuid,
    action: Action,
    target: Option<Uuid>,
    expected: Option<Snapshot>,
}

#[derive(Serialize)]
struct Plan {
    location: Location,
    target: Option<Location>,
    expected: Snapshot,
    action: Action,
}

fn reason(value: &str) -> Result<&str, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() || trimmed.chars().count() > 500 {
        return Err(bad_request("Enter a review reason of 1–500 characters."));
    }
    Ok(trimmed)
}

fn curator_required() -> AppError {
    AppError(
        StatusCode::FORBIDDEN,
        "Catalog curator access is required.".into(),
    )
}

async fn review_plan(
    db: &mut PgConnection,
    user: &auth::User,
    review: &Review,
    applying: bool,
) -> Result<Vec<Plan>, AppError> {
    reason(&review.reason)?;
    if review.curator && !user.is_admin {
        return Err(curator_required());
    }
    if review.changes.is_empty() || review.changes.len() > MAX_BATCH {
        return Err(bad_request("Submit 1–50 distinct locations per review."));
    }
    let changed_anchors: HashSet<_> = review
        .changes
        .iter()
        .filter(|change| !matches!(change.action, Action::Decline))
        .map(|change| change.location_id)
        .collect();
    let mut seen = HashSet::new();
    let mut plans = Vec::with_capacity(review.changes.len());
    for change in &review.changes {
        if !seen.insert(change.location_id) {
            return Err(bad_request("A location occurs more than once."));
        }
        if matches!(change.action, Action::Split) && !review.curator {
            return Err(curator_required());
        }
        let own = location(db, user.id, change.location_id, !review.curator).await?;
        if own.scope.as_deref() == Some("curator")
            && !review.curator
            && !matches!(change.action, Action::Decline)
        {
            return Err(curator_required());
        }
        let target = match (change.action, change.target) {
            (Action::Link | Action::Decline, Some(id)) if id != change.location_id => {
                let other = location(db, user.id, id, false).await?;
                if own.kind != other.kind || own.representation_id == other.representation_id {
                    return Err(bad_request(
                        "Choose a different representation of the same media type.",
                    ));
                }
                if matches!(change.action, Action::Link)
                    && (other.active_link.is_some() || changed_anchors.contains(&id))
                {
                    return Err(bad_request(
                        "Choose the anchor representation for the group.",
                    ));
                }
                if matches!(change.action, Action::Link)
                    && own
                        .entity_type
                        .as_ref()
                        .zip(other.entity_type.as_ref())
                        .is_some_and(|(own_type, target_type)| own_type != target_type)
                {
                    return Err(bad_request(
                        "The locations describe different media entities.",
                    ));
                }
                if matches!(change.action, Action::Link)
                    && own.canonical_id.is_some()
                    && own.canonical_id != other.canonical_id
                {
                    return Err(bad_request(
                        "This link conflicts with the verified identity.",
                    ));
                }
                Some(other)
            }
            (Action::Unlink | Action::Split, None) => None,
            _ => {
                return Err(bad_request(
                    "A link or decline requires a different target location.",
                ))
            }
        };
        let expected = Snapshot {
            representation_id: own.representation_id,
            revision: own.revision,
            target_representation_id: target.as_ref().map(|row| row.representation_id),
            identity: own.canonical_id,
            target_identity: target.as_ref().and_then(|row| row.canonical_id),
            target_revision: target.as_ref().and_then(|row| row.revision),
        };
        if applying && change.expected.as_ref() != Some(&expected) {
            return Err(conflict());
        }
        plans.push(Plan {
            location: own,
            target,
            expected,
            action: change.action,
        });
    }
    Ok(plans)
}

async fn preview(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(review): Json<Review>,
) -> Result<Response, AppError> {
    let actor = identity.user.id;
    auth::rate_limit(&state.db, format!("catalog:preview:{actor}"), 30, 300).await?;
    let mut db = state.db.acquire().await.map_err(|_| auth::unavailable())?;
    Ok(private(
        review_plan(&mut db, &identity.user, &review, false).await?,
    ))
}

async fn transaction(state: &AppState) -> Result<Transaction<'_, Postgres>, AppError> {
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    // All review writers use this lock. Use try-lock so a request cannot queue
    // indefinitely or proceed with a snapshot from before the competing write.
    if !sqlx::query_scalar::<_, bool>("SELECT pg_try_advisory_xact_lock($1)")
        .bind(LOCK)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?
    {
        return Err(conflict());
    }
    Ok(tx)
}

async fn apply(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(review): Json<Review>,
) -> Result<Response, AppError> {
    let actor = identity.user.id;
    auth::rate_limit(&state.db, format!("catalog:write:{actor}"), 12, 300).await?;
    let mut tx = transaction(&state).await?;
    let plans = review_plan(&mut tx, &identity.user, &review, true).await?;
    let batch = Uuid::new_v4();
    let scope = if review.curator { "curator" } else { "owner" };
    let mut changed = 0;
    for plan in &plans {
        // Avoid redundant decisions and duplicate decline cases.
        let result = sqlx::query(
            "INSERT INTO catalog_review_events
                 (batch_id,location_id,representation_id,target_representation_id,actor_id,
                  scope,action,reason,previous_id,candidate_signature,suppress_verified)
             SELECT $1,$2,$3,$4,$5,$6,$7,$8,$9,
                    CASE WHEN $7='decline' THEN md5(jsonb_build_array($10::text,$11::text)::text) END,
                    $7='split'
             WHERE NOT EXISTS (
                 SELECT 1 FROM catalog_review_events e
                 WHERE e.location_id=$2 AND e.representation_id=$3 AND e.action=$7 AND
                   ( ($7='decline' AND e.target_representation_id=$4
                         AND e.candidate_signature=md5(jsonb_build_array($10::text,$11::text)::text))
                     OR ($7<>'decline' AND e.id=$9 AND e.target_representation_id IS NOT DISTINCT FROM $4)))",
        )
        .bind(batch)
        .bind(plan.location.location_id)
        .bind(plan.location.representation_id)
        .bind(plan.expected.target_representation_id)
        .bind(actor)
        .bind(scope)
        .bind(plan.action.as_str())
        .bind(review.reason.trim())
        .bind(plan.location.revision)
        .bind(&plan.location.name)
        .bind(plan.target.as_ref().map(|row| row.name.as_str()))
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
        changed += result.rows_affected();
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(private(
        serde_json::json!({"batch":batch,"changed":changed}),
    ))
}

#[derive(FromRow)]
struct Event {
    id: i64,
    location_id: Uuid,
    representation_id: Uuid,
    previous_id: Option<i64>,
    scope: String,
    actor_id: Option<Uuid>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Reason {
    reason: String,
}

async fn undo(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(batch): Path<Uuid>,
    Json(input): Json<Reason>,
) -> Result<Response, AppError> {
    let reason = reason(&input.reason)?;
    let actor = identity.user.id;
    auth::rate_limit(&state.db, format!("catalog:write:{actor}"), 12, 300).await?;
    let mut tx = transaction(&state).await?;
    let events = sqlx::query_as::<_, Event>(
        "SELECT id,location_id,representation_id,previous_id,scope,actor_id
         FROM catalog_review_events
         WHERE batch_id=$1 AND action IN ('link','unlink','split') ORDER BY id LIMIT 50",
    )
    .bind(batch)
    .fetch_all(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if events.is_empty() {
        return Err(missing());
    }
    let reversal = Uuid::new_v4();
    for event in events {
        let curator = event.scope == "curator";
        if (curator && !identity.user.is_admin) || (!curator && event.actor_id != Some(actor)) {
            return Err(missing());
        }
        let current = location(&mut tx, actor, event.location_id, !curator).await?;
        if current.representation_id != event.representation_id
            || current.revision != Some(event.id)
        {
            return Err(conflict());
        }
        sqlx::query(
            "INSERT INTO catalog_review_events
                (batch_id,location_id,representation_id,actor_id,scope,action,reason,previous_id,
                 effective_link_id,suppress_verified)
             SELECT $1,$2,$3,$4,$5,'undo',$6,$7,
                    CASE WHEN prior.action='link' THEN prior.id ELSE prior.effective_link_id END,
                    COALESCE(prior.suppress_verified,false)
             FROM (SELECT 1) row LEFT JOIN catalog_review_events prior ON prior.id=$8",
        )
        .bind(reversal)
        .bind(current.location_id)
        .bind(current.representation_id)
        .bind(actor)
        .bind(event.scope)
        .bind(reason)
        .bind(event.id)
        .bind(event.previous_id)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(private(serde_json::json!({"batch":reversal})))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Verification {
    location_id: Uuid,
    representation_id: Uuid,
    provider: Provider,
    id: String,
    season: Option<u32>,
    episode: Option<u32>,
    edition: String,
    reason: String,
}

fn identity_key(input: &Verification) -> Result<(&'static str, String, Fields), AppError> {
    let invalid = || bad_request("Specify a valid provider identifier at the file's entity level.");
    if !input.provider.validate_id(&input.id) {
        return Err(invalid());
    }
    let mut fields = Fields::default();
    let (entity, identifier) = match (input.provider, input.season, input.episode) {
        (Provider::TmdbMovie, None, None) => (
            "movie",
            input.id.parse::<u64>().map_err(|_| invalid())?.to_string(),
        ),
        (Provider::TmdbTv, Some(season), Some(episode))
            if season <= 100_000 && (1..=100_000).contains(&episode) =>
        {
            fields.season = Some(season);
            fields.episode = Some(episode);
            let series = input.id.parse::<u64>().map_err(|_| invalid())?;
            ("episode", format!("{series}:{season}:{episode}"))
        }
        (Provider::Musicbrainz, None, None) => (
            "recording",
            Uuid::parse_str(&input.id)
                .map_err(|_| invalid())?
                .to_string(),
        ),
        (Provider::Openlibrary, None, None) => ("work", input.id.clone()),
        _ => return Err(invalid()),
    };
    Ok((entity, identifier, fields))
}

async fn verify(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(input): Json<Verification>,
) -> Result<Response, AppError> {
    if !identity.user.is_admin {
        return Err(curator_required());
    }
    let actor = identity.user.id;
    let note = reason(&input.reason)?;
    let edition = input.edition.trim().to_ascii_lowercase();
    if edition.is_empty()
        || edition.len() > 80
        || !edition
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._:".contains(&b))
    {
        return Err(bad_request(
            "Provide an explicit, stable edition or cut key.",
        ));
    }
    let (entity, identifier, context) = identity_key(&input)?;
    let before = {
        let mut db = state.db.acquire().await.map_err(|_| auth::unavailable())?;
        location(&mut db, actor, input.location_id, false).await?
    };
    if before.representation_id != input.representation_id {
        return Err(conflict());
    }
    let _permit = metadata::online_guard(&state, actor, input.provider, &before.kind).await?;
    let provider = input.provider.name();
    let verified =
        metadata::providers::preview(&state, input.provider, &input.id, &context).await?;
    if verified.fields.title.is_none() {
        return Err(bad_request(
            "The provider did not confirm the requested entity.",
        ));
    }
    let artwork = match verified.art {
        Some(ref art) => metadata::providers::artwork(&state, art).await.ok(),
        None => None,
    };
    let mut tx = transaction(&state).await?;
    let current = location(&mut tx, actor, input.location_id, false).await?;
    if before.representation_id != current.representation_id || before.revision != current.revision
    {
        return Err(conflict());
    }
    let canonical = attest(
        &mut tx,
        &current,
        provider,
        entity,
        &identifier,
        &edition,
        actor,
        note,
    )
    .await;
    match canonical {
        Ok(id) => {
            let artwork_id = metadata::store_art(&mut tx, artwork).await?;
            sqlx::query(
                "UPDATE catalog_verified_identities SET provider_fields=$2,
                         provider_art=COALESCE($3,provider_art) WHERE representation_id=$1",
            )
            .bind(current.representation_id)
            .bind(serde_json::to_value(&verified.fields).map_err(|_| auth::unavailable())?)
            .bind(artwork_id)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
            tx.commit().await.map_err(|_| auth::unavailable())?;
            Ok(private(serde_json::json!({"id":id})))
        }
        Err(error) => {
            if error.0 == StatusCode::CONFLICT {
                tx.commit().await.map_err(|_| auth::unavailable())?;
            }
            Err(error)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn attest(
    tx: &mut PgConnection,
    current: &Location,
    provider: &str,
    entity: &str,
    identifier: &str,
    edition: &str,
    actor: Uuid,
    note: &str,
) -> Result<Uuid, AppError> {
    let existing: Option<(String, String, String, String, Uuid)> = sqlx::query_as(
        "SELECT namespace,entity_type,identifier,edition,canonical_id
         FROM catalog_verified_identities WHERE representation_id=$1",
    )
    .bind(current.representation_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    let conflict =
        if let Some((old_provider, old_entity, old_identifier, old_edition, id)) = existing {
            if (
                old_provider.as_str(),
                old_entity.as_str(),
                old_identifier.as_str(),
                old_edition.as_str(),
            ) == (provider, entity, identifier, edition)
            {
                return Ok(id);
            }
            !current.suppress_verified
        } else {
            false
        };
    let evidence_id: Uuid = sqlx::query_scalar(
        "INSERT INTO catalog_identity_evidence(id,location_id,origin,namespace,entity_type,identifier,verification)
         VALUES ($1,$2,'provider_curator',$3,$4,$5,'verified')
         ON CONFLICT(location_id,origin,namespace,entity_type,identifier)
         DO UPDATE SET observed_at=now() RETURNING id",
    )
    .bind(Uuid::new_v4()).bind(current.location_id).bind(provider).bind(entity).bind(identifier)
    .fetch_one(&mut *tx).await.map_err(|_| auth::unavailable())?;
    let evidence = serde_json::json!({"provider":provider,"entity":entity,"identifier":identifier,"edition":edition});
    if conflict {
        sqlx::query(
            "INSERT INTO catalog_review_events(batch_id,location_id,representation_id,actor_id,scope,
                action,reason,previous_id,verification_evidence_id,evidence_snapshot)
             SELECT $1,$2,$3,$4,'curator','verification_conflict',$5,$6,$7,$8
             WHERE NOT EXISTS (SELECT 1 FROM catalog_review_events WHERE location_id=$2
                 AND representation_id=$3 AND action='verification_conflict'
                 AND previous_id IS NOT DISTINCT FROM $6 AND evidence_snapshot=$8)",
        )
        .bind(Uuid::new_v4()).bind(current.location_id).bind(current.representation_id).bind(actor)
        .bind(note).bind(current.revision).bind(evidence_id).bind(&evidence)
        .execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
        return Err(AppError(
            StatusCode::CONFLICT,
            "Conflicting verified identity. Use a curator split before correction.".into(),
        ));
    }
    let id = sqlx::query_scalar::<_, Uuid>(
        "SELECT item_id FROM catalog_identity_groups WHERE namespace=$1 AND entity_type=$2
            AND identifier=$3 AND edition=$4",
    )
    .bind(provider)
    .bind(entity)
    .bind(identifier)
    .bind(edition)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    let id = if let Some(id) = id {
        id
    } else {
        let id = Uuid::new_v4();
        sqlx::query("INSERT INTO catalog_items(id,kind) VALUES ($1,$2)")
            .bind(id)
            .bind(&current.kind)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
        sqlx::query("INSERT INTO catalog_identity_groups VALUES ($1,$2,$3,$4,$5)")
            .bind(provider)
            .bind(entity)
            .bind(identifier)
            .bind(edition)
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
        id
    };
    sqlx::query(
        "INSERT INTO catalog_verified_identities
         VALUES ($1,$2,$3,$4,$5,$6,$7)
         ON CONFLICT(representation_id) DO UPDATE SET namespace=EXCLUDED.namespace,
            entity_type=EXCLUDED.entity_type,identifier=EXCLUDED.identifier,edition=EXCLUDED.edition,
            canonical_id=EXCLUDED.canonical_id,evidence_id=EXCLUDED.evidence_id,
            provider_fields=NULL,provider_art=NULL",
    )
    .bind(current.representation_id).bind(provider).bind(entity).bind(identifier)
    .bind(edition).bind(id).bind(evidence_id).execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
    sqlx::query(
        "INSERT INTO catalog_review_events(batch_id,location_id,representation_id,actor_id,scope,
            action,reason,previous_id,verified_identity_id,verification_evidence_id,evidence_snapshot)
         VALUES ($1,$2,$3,$4,'curator','verify',$5,$6,$7,$8,$9)",
    )
    .bind(Uuid::new_v4())
    .bind(current.location_id)
    .bind(current.representation_id)
    .bind(actor)
    .bind(note)
    .bind(current.revision)
    .bind(id)
    .bind(evidence_id)
    .bind(evidence)
    .execute(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    Ok(id)
}

async fn report(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    Json(input): Json<Reason>,
) -> Result<Response, AppError> {
    let reason = reason(&input.reason)?;
    let actor = identity.user.id;
    auth::rate_limit(&state.db, format!("catalog:report:{actor}"), 5, 3600).await?;
    let mut tx = transaction(&state).await?;
    let visible = location(&mut tx, actor, id, false).await?;
    let queued: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO catalog_review_cases(id,source_id,location_id,representation_id,reporter_id,reason)
         SELECT $1,$2,$3,$4,$5,$6
         WHERE NOT EXISTS(SELECT 1 FROM catalog_review_suppressions WHERE source_id=$2)
           AND (SELECT count(*) FROM catalog_review_cases c JOIN catalog_locations l
                ON l.id=c.location_id AND l.representation_id=c.representation_id
                WHERE c.source_id=$2 AND c.resolved_at IS NULL
                AND c.created_at>now()-interval '30 days')<20
           AND (SELECT count(*) FROM catalog_review_cases c JOIN catalog_locations l
                ON l.id=c.location_id AND l.representation_id=c.representation_id
                WHERE c.resolved_at IS NULL AND c.created_at>now()-interval '30 days')<500
         ON CONFLICT(location_id,representation_id) DO NOTHING RETURNING id",
    )
    .bind(Uuid::new_v4()).bind(&visible.source_id).bind(id).bind(visible.representation_id)
    .bind(actor).bind(reason).fetch_optional(&mut *tx).await.map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(private(serde_json::json!({"queued":queued.is_some()})))
}

#[derive(Serialize, FromRow)]
struct Case {
    id: Uuid,
    location_id: Uuid,
    representation_id: Uuid,
    reason: String,
}

async fn cases(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Query(page): Query<Page>,
) -> Result<Response, AppError> {
    if !identity.user.is_admin {
        return Err(curator_required());
    }
    let limit = page.limit.unwrap_or(30).clamp(1, 100) as usize;
    let mut rows = sqlx::query_as::<_, Case>(
        "SELECT c.id,c.location_id,c.representation_id,c.reason FROM catalog_review_cases c
         JOIN catalog_discoverable_locations v ON v.location_id=c.location_id
             AND v.representation_id=c.representation_id AND v.viewer_id=$1
         WHERE c.resolved_at IS NULL AND c.created_at>now()-interval '30 days'
             AND ($2::uuid IS NULL OR c.id>$2)
         ORDER BY c.id LIMIT $3",
    )
    .bind(identity.user.id)
    .bind(page.after)
    .bind((limit + 1) as i64)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    let more = rows.len() > limit;
    rows.truncate(limit);
    let next = more.then(|| rows.last().map(|row| row.id)).flatten();
    Ok(private(serde_json::json!({"cases":rows,"next":next})))
}

async fn resolve_case(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    Json(input): Json<Reason>,
) -> Result<StatusCode, AppError> {
    if !identity.user.is_admin {
        return Err(curator_required());
    }
    let note = reason(&input.reason)?;
    let mut tx = transaction(&state).await?;
    let source: String = sqlx::query_scalar(
        "UPDATE catalog_review_cases c SET resolved_at=now()
         WHERE c.id=$1 AND c.resolved_at IS NULL AND EXISTS(
            SELECT 1 FROM catalog_discoverable_locations v WHERE v.viewer_id=$2
                AND v.location_id=c.location_id AND v.representation_id=c.representation_id)
         RETURNING source_id",
    )
    .bind(id)
    .bind(identity.user.id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(missing)?;
    sqlx::query(
        "INSERT INTO catalog_curator_audit(actor_id,source_id,case_id,action,reason)
                 VALUES($1,$2,$3,'resolve',$4)",
    )
    .bind(identity.user.id)
    .bind(source)
    .bind(id)
    .bind(note)
    .execute(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Suppression {
    suppressed: bool,
    reason: String,
}

async fn suppress(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(input): Json<Suppression>,
) -> Result<StatusCode, AppError> {
    if !identity.user.is_admin {
        return Err(curator_required());
    }
    let note = reason(&input.reason)?;
    let mut tx = transaction(&state).await?;
    let allowed = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM catalog_discoverable_locations WHERE viewer_id=$1 AND source_id=$2)",
    ).bind(identity.user.id).bind(&source).fetch_one(&mut *tx).await.map_err(|_| auth::unavailable())?;
    if !allowed {
        return Err(missing());
    }
    let (query, action) = if input.suppressed {
        (
            "INSERT INTO catalog_review_suppressions(source_id) VALUES($1) ON CONFLICT DO NOTHING",
            "suppress",
        )
    } else {
        (
            "DELETE FROM catalog_review_suppressions WHERE source_id=$1",
            "unsuppress",
        )
    };
    let changed = sqlx::query(query)
        .bind(&source)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    if changed.rows_affected() != 0 {
        sqlx::query(
            "INSERT INTO catalog_curator_audit(actor_id,source_id,action,reason)
                     VALUES($1,$2,$3,$4)",
        )
        .bind(identity.user.id)
        .bind(source)
        .bind(action)
        .bind(note)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests;
