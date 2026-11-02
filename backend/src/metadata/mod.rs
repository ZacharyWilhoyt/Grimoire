mod local;
pub mod model;
mod providers;
use crate::{auth, catalog, AppError, AppState};
use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    Extension, Json,
};
use model::{ArtworkPreference, Fields};
use providers::Provider;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, Postgres, Transaction};
use std::{
    collections::{HashMap, HashSet},
    path::PathBuf,
    sync::Arc,
    time::Duration,
};
use tokio::sync::Semaphore;
use uuid::Uuid;

pub struct Runtime {
    pub client: reqwest::Client,
    pub online: bool,
    pub tmdb_token: Option<String>,
    work: Arc<Semaphore>,
    jobs: Arc<Semaphore>,
}
impl Runtime {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::builder()
                .https_only(true)
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .user_agent(
                    std::env::var("GRIMOIRE_METADATA_USER_AGENT").unwrap_or_else(|_| {
                        "Grimoire/0.1 (+https://github.com/ZacharyWilhoyt/Grimoire)".into()
                    }),
                )
                .build()
                .expect("Metadata HTTP client configuration failed"),
            online: std::env::var("GRIMOIRE_METADATA_ONLINE").as_deref() != Ok("false"),
            tmdb_token: std::env::var("GRIMOIRE_TMDB_TOKEN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            work: Arc::new(Semaphore::new(2)),
            jobs: Arc::new(Semaphore::new(1)),
        }
    }
}
#[derive(Clone, FromRow)]
struct Record {
    file_key: String,
    source_id: String,
    relative_path: String,
    local_data: String,
    matched_data: String,
    custom_data: Option<String>,
    provider: Option<String>,
    provider_id: Option<String>,
    local_artwork: Option<String>,
    matched_artwork: Option<String>,
    artwork_preference: String,
    extract_stamp: Option<String>,
}
#[derive(Clone, Serialize)]
pub struct View {
    pub fields: Fields,
    pub has_custom: bool,
    pub origin: String,
    pub artwork_url: Option<String>,
    pub can_edit: bool,
    pub artwork_preference: ArtworkPreference,
    pub provider: Option<String>,
    pub provider_id: Option<String>,
}
fn decode(value: &str) -> Fields {
    serde_json::from_str(value).unwrap_or_default()
}
fn art_id(record: &Record) -> Option<&str> {
    match record.artwork_preference.as_str() {
        "none" => None,
        "local" => record.local_artwork.as_deref(),
        "matched" => record.matched_artwork.as_deref(),
        _ => record
            .matched_artwork
            .as_deref()
            .or(record.local_artwork.as_deref()),
    }
}
fn view(path: &str, key: &str, record: Option<&Record>, can_edit: bool, available: bool) -> View {
    let fallback = local::filename_fields(path);
    let (local, matched, custom) = record
        .map(|record| {
            (
                decode(&record.local_data),
                decode(&record.matched_data),
                record.custom_data.as_deref().map(decode),
            )
        })
        .unwrap_or((fallback, Fields::default(), None));
    View {
        fields: model::effective(path, &local, &matched, custom.as_ref()),
        has_custom: custom.is_some(),
        origin: if custom.is_some() {
            "custom".into()
        } else {
            record
                .and_then(|record| record.provider.clone())
                .unwrap_or_else(|| {
                    if record.is_some() {
                        "local".into()
                    } else {
                        "filename".into()
                    }
                })
        },
        artwork_url: available
            .then(|| {
                record
                    .and_then(art_id)
                    .map(|id| format!("/api/metadata/artwork/{key}?v={id}"))
            })
            .flatten(),
        can_edit,
        artwork_preference: record
            .and_then(|record| {
                serde_json::from_value(serde_json::Value::String(record.artwork_preference.clone()))
                    .ok()
            })
            .unwrap_or_default(),
        provider: record.and_then(|record| record.provider.clone()),
        provider_id: record.and_then(|record| record.provider_id.clone()),
    }
}
pub async fn enrich(
    state: &AppState,
    user: Uuid,
    items: &mut [catalog::Item],
) -> Result<(), AppError> {
    let keys: Vec<_> = items.iter().map(|item| item.file_key.clone()).collect();
    let sources: Vec<_> = items
        .iter()
        .map(|item| item.source_id.clone())
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let owned: Vec<String> =
        sqlx::query_scalar("SELECT id FROM sources WHERE owner_id=$1 AND id=ANY($2)")
            .bind(user)
            .bind(sources)
            .fetch_all(&state.db)
            .await
            .map_err(|_| auth::unavailable())?;
    let owned: HashSet<_> = owned.into_iter().collect();
    let rows=sqlx::query_as::<_,Record>("SELECT m.* FROM media_metadata m JOIN sources s ON s.id=m.source_id WHERE m.file_key=ANY($2) AND (s.owner_id=$1 OR EXISTS(SELECT 1 FROM source_permissions p WHERE p.source_id=s.id AND p.user_id=$1 AND p.can_discover AND p.can_access_media))").bind(user).bind(keys).fetch_all(&state.db).await.map_err(|_|auth::unavailable())?;
    let records: HashMap<_, _> = rows
        .into_iter()
        .map(|record| (record.file_key.clone(), record))
        .collect();
    for item in items {
        let mut metadata = view(
            &item.file.relative_path,
            &item.file_key,
            records.get(&item.file_key),
            owned.contains(&item.source_id),
            item.available,
        );
        metadata.fields.description = None;
        item.metadata = Some(metadata);
    }
    Ok(())
}
#[derive(Deserialize)]
pub struct FileQuery {
    pub path: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileBody {
    pub path: String,
}
struct Reference {
    key: String,
    path: PathBuf,
    kind: &'static str,
}
async fn reference(
    state: &AppState,
    user: &auth::User,
    source: &str,
    path: &str,
    owner: bool,
) -> Result<Reference, AppError> {
    if path.len() > 4096 {
        return Err(crate::bad_request("Invalid file path."));
    }
    let root = if owner {
        let path:Option<String>=sqlx::query_scalar("SELECT s.path FROM sources s JOIN users u ON u.id=s.owner_id WHERE s.id=$1 AND s.owner_id=$2 AND NOT u.disabled").bind(source).bind(user.id).fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?;
        PathBuf::from(path.ok_or_else(not_found)?)
    } else {
        crate::media_root(state, user, source).await?
    };
    let extension = std::path::Path::new(path)
        .extension()
        .and_then(|value| value.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let kind = crate::classify_extension(&extension).ok_or_else(not_found)?;
    let canonical = crate::resolve_media_path(&root, path, kind)?;
    if canonical
        .strip_prefix(&root)
        .ok()
        .and_then(|path| path.to_str())
        != Some(path)
    {
        return Err(crate::bad_request("Use the original catalog file path."));
    }
    Ok(Reference {
        key: catalog::file_key(source, path),
        path: canonical,
        kind,
    })
}
fn not_found() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Source or media not found.".into())
}
async fn load(state: &AppState, key: &str) -> Result<Option<Record>, AppError> {
    sqlx::query_as::<_, Record>("SELECT * FROM media_metadata WHERE file_key=$1")
        .bind(key)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| auth::unavailable())
}
async fn owner_lock(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    owner: Uuid,
) -> Result<(), AppError> {
    let id:Option<Uuid>=sqlx::query_scalar("SELECT s.owner_id FROM sources s JOIN users u ON u.id=s.owner_id WHERE s.id=$1 AND NOT u.disabled FOR UPDATE OF s").bind(source).fetch_optional(&mut **tx).await.map_err(|_|auth::unavailable())?;
    if id != Some(owner) {
        return Err(not_found());
    }
    Ok(())
}
async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    key: &str,
    owner: Uuid,
    action: &str,
    before: Option<String>,
    after: Option<String>,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO metadata_audit(source_id,file_key,actor_id,action,before_data,after_data) VALUES ($1,$2,$3,$4,$5,$6)").bind(source).bind(key).bind(owner).bind(action).bind(before).bind(after).execute(&mut **tx).await.map_err(|_|auth::unavailable())?;
    Ok(())
}
async fn ensure_row(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    path: &str,
    key: &str,
) -> Result<(), AppError> {
    sqlx::query("INSERT INTO media_metadata(file_key,source_id,relative_path) VALUES ($1,$2,$3) ON CONFLICT(file_key) DO NOTHING").bind(key).bind(source).bind(path).execute(&mut **tx).await.map_err(|_|auth::unavailable())?;
    Ok(())
}
async fn store_art(
    tx: &mut Transaction<'_, Postgres>,
    bytes: Option<Vec<u8>>,
) -> Result<Option<String>, AppError> {
    if let Some(bytes) = bytes {
        if bytes.len() > 524288 {
            return Err(crate::bad_request("Artwork exceeds its size limit."));
        }
        let id = hex::encode(Sha256::digest(&bytes));
        sqlx::query(
            "INSERT INTO metadata_artwork(id,jpeg) VALUES ($1,$2) ON CONFLICT(id) DO NOTHING",
        )
        .bind(&id)
        .bind(bytes)
        .execute(&mut **tx)
        .await
        .map_err(|_| auth::unavailable())?;
        Ok(Some(id))
    } else {
        Ok(None)
    }
}
async fn current_view(
    state: &AppState,
    user: &auth::User,
    source: &str,
    path: &str,
    key: &str,
) -> Result<View, AppError> {
    let owner: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE id=$1 AND owner_id=$2)")
            .bind(source)
            .bind(user.id)
            .fetch_one(&state.db)
            .await
            .map_err(|_| auth::unavailable())?;
    Ok(view(
        path,
        key,
        load(state, key).await?.as_ref(),
        owner,
        true,
    ))
}
pub async fn details(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<View>, AppError> {
    let file = reference(&state, &identity.user, &source, &query.path, false).await?;
    Ok(Json(
        current_view(&state, &identity.user, &source, &query.path, &file.key).await?,
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Edit {
    path: String,
    fields: Fields,
    #[serde(default)]
    artwork_preference: ArtworkPreference,
}
pub async fn edit(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(edit): Json<Edit>,
) -> Result<Json<View>, AppError> {
    let file = reference(&state, &identity.user, &source, &edit.path, true).await?;
    let fields = edit.fields.normalize()?;
    let data = serde_json::to_string(&fields).map_err(|_| auth::unavailable())?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    ensure_row(&mut tx, &source, &edit.path, &file.key).await?;
    let before: Option<String> =
        sqlx::query_scalar("SELECT custom_data FROM media_metadata WHERE file_key=$1")
            .bind(&file.key)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    sqlx::query("UPDATE media_metadata SET custom_data=$2,artwork_preference=$3,updated_at=now() WHERE file_key=$1").bind(&file.key).bind(&data).bind(edit.artwork_preference.as_str()).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    audit(
        &mut tx,
        &source,
        &file.key,
        identity.user.id,
        "custom_edited",
        before,
        Some(data),
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        current_view(&state, &identity.user, &source, &edit.path, &file.key).await?,
    ))
}
pub async fn reset(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Query(query): Query<FileQuery>,
) -> Result<Json<View>, AppError> {
    let file = reference(&state, &identity.user, &source, &query.path, true).await?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    let before: Option<String> =
        sqlx::query_scalar("SELECT custom_data FROM media_metadata WHERE file_key=$1")
            .bind(&file.key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?
            .flatten();
    sqlx::query("UPDATE media_metadata SET custom_data=NULL,artwork_preference='auto',updated_at=now() WHERE file_key=$1").bind(&file.key).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    audit(
        &mut tx,
        &source,
        &file.key,
        identity.user.id,
        "custom_reset",
        before,
        None,
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        current_view(&state, &identity.user, &source, &query.path, &file.key).await?,
    ))
}
fn stamp(path: &std::path::Path) -> Result<String, AppError> {
    let metadata = path.metadata().map_err(|_| not_found())?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|time| time.as_nanos())
        .unwrap_or_default();
    Ok(format!("{}:{modified}", metadata.len()))
}
async fn extract_one(
    state: &AppState,
    user: &auth::User,
    source: &str,
    path: &str,
    force: bool,
) -> Result<Option<Vec<String>>, AppError> {
    let file = reference(state, user, source, path, true).await?;
    let initial = stamp(&file.path)?;
    let previous = load(state, &file.key).await?;
    if !force
        && previous
            .as_ref()
            .and_then(|record| record.extract_stamp.as_ref())
            == Some(&initial)
    {
        return Ok(None);
    }
    let extracted = local::extract(&file.path, file.kind).await?;
    if stamp(&file.path)? != initial {
        return Err(AppError(
            StatusCode::CONFLICT,
            "The file changed during metadata extraction. Try again after it finishes writing."
                .into(),
        ));
    }
    let data = serde_json::to_string(&extracted.fields).map_err(|_| auth::unavailable())?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, source, user.id).await?;
    ensure_row(&mut tx, source, path, &file.key).await?;
    let artwork = store_art(&mut tx, extracted.artwork).await?;
    sqlx::query("UPDATE media_metadata SET local_data=$2,local_artwork=$3,extract_stamp=$4,updated_at=now() WHERE file_key=$1").bind(&file.key).bind(&data).bind(artwork).bind(initial).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    audit(
        &mut tx,
        source,
        &file.key,
        user.id,
        "local_extracted",
        previous.map(|record| record.local_data),
        Some(data),
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Some(extracted.warnings))
}
#[derive(Serialize)]
pub struct ExtractionResponse {
    pub metadata: View,
    pub warnings: Vec<String>,
}
pub async fn extract(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(body): Json<FileBody>,
) -> Result<Json<ExtractionResponse>, AppError> {
    let _permit = state
        .metadata
        .work
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::TOO_MANY_REQUESTS,
                "Metadata processing is busy. Try again shortly.".into(),
            )
        })?;
    let warnings = extract_one(&state, &identity.user, &source, &body.path, true)
        .await?
        .unwrap_or_default();
    let key = catalog::file_key(&source, &body.path);
    Ok(Json(ExtractionResponse {
        metadata: current_view(&state, &identity.user, &source, &body.path, &key).await?,
        warnings,
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Search {
    path: String,
    provider: Provider,
    query: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    path: String,
    provider: Provider,
    id: String,
}
#[derive(Serialize)]
pub struct Review {
    pub review_token: Uuid,
    pub provider: Provider,
    pub provider_id: String,
    pub fields: Fields,
    pub has_artwork: bool,
}
async fn online_guard(
    state: &AppState,
    user: Uuid,
    provider: Provider,
    kind: &str,
) -> Result<tokio::sync::OwnedSemaphorePermit, AppError> {
    if !provider.supports(kind) {
        return Err(crate::bad_request(
            "That provider does not support this media type.",
        ));
    }
    if !providers::enabled(state, provider) {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "Online lookup is disabled or this provider needs server credentials.".into(),
        ));
    }
    auth::rate_limit(&state.db, format!("metadata:user:{user}"), 30, 300).await?;
    state
        .metadata
        .work
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::TOO_MANY_REQUESTS,
                "Metadata processing is busy. Try again shortly.".into(),
            )
        })
}
pub async fn provider_status(State(state): State<AppState>) -> Json<Vec<providers::Availability>> {
    Json(providers::availability(&state))
}
pub async fn search(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(search): Json<Search>,
) -> Result<Json<Vec<providers::Candidate>>, AppError> {
    let file = reference(&state, &identity.user, &source, &search.path, true).await?;
    let query = search.query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err(crate::bad_request("Enter a search of 1–200 characters."));
    }
    let _permit = online_guard(&state, identity.user.id, search.provider, file.kind).await?;
    let result = providers::search(&state, search.provider, query).await?;
    reference(&state, &identity.user, &source, &search.path, true).await?;
    Ok(Json(result))
}
pub async fn preview(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(preview): Json<Preview>,
) -> Result<Json<Review>, AppError> {
    let file = reference(&state, &identity.user, &source, &preview.path, true).await?;
    let _permit = online_guard(&state, identity.user.id, preview.provider, file.kind).await?;
    let context = current_view(&state, &identity.user, &source, &preview.path, &file.key)
        .await?
        .fields;
    let matched = providers::preview(&state, preview.provider, &preview.id, &context).await?;
    let id = Uuid::new_v4();
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    sqlx::query("DELETE FROM metadata_reviews WHERE expires_at<now()")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    sqlx::query("INSERT INTO metadata_reviews(id,owner_id,source_id,file_key,provider,provider_id,data,artwork_ref) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)").bind(id).bind(identity.user.id).bind(source).bind(file.key).bind(preview.provider.name()).bind(&preview.id).bind(serde_json::to_string(&matched.fields).map_err(|_|auth::unavailable())?).bind(matched.art.as_ref().map(serde_json::to_string).transpose().map_err(|_|auth::unavailable())?).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(Review {
        review_token: id,
        provider: preview.provider,
        provider_id: preview.id,
        fields: matched.fields,
        has_artwork: matched.art.is_some(),
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Apply {
    path: String,
    review_token: Uuid,
    #[serde(default)]
    replace_custom: bool,
}
#[derive(FromRow)]
struct ReviewRow {
    provider: String,
    provider_id: String,
    data: String,
    artwork_ref: Option<String>,
}
pub async fn apply(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(apply): Json<Apply>,
) -> Result<Json<ExtractionResponse>, AppError> {
    let file = reference(&state, &identity.user, &source, &apply.path, true).await?;
    let _permit = state
        .metadata
        .work
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::TOO_MANY_REQUESTS,
                "Metadata processing is busy.".into(),
            )
        })?;
    let review=sqlx::query_as::<_,ReviewRow>("SELECT provider,provider_id,data,artwork_ref FROM metadata_reviews WHERE id=$1 AND owner_id=$2 AND source_id=$3 AND file_key=$4 AND expires_at>now()").bind(apply.review_token).bind(identity.user.id).bind(&source).bind(&file.key).fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?.ok_or_else(||crate::bad_request("This review expired or belongs to a different file/account. Preview the match again."))?;
    let provider: Provider =
        serde_json::from_value(serde_json::Value::String(review.provider.clone()))
            .map_err(|_| auth::unavailable())?;
    if !providers::enabled(&state, provider) {
        return Err(AppError(
            StatusCode::SERVICE_UNAVAILABLE,
            "This provider is disabled.".into(),
        ));
    }
    let mut warnings = Vec::new();
    let artwork = if let Some(art) = review
        .artwork_ref
        .as_deref()
        .and_then(|value| serde_json::from_str::<providers::Art>(value).ok())
    {
        match providers::artwork(&state, &art).await {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                warnings.push("The match was applied, but provider artwork was unavailable. Existing local artwork remains available.".into());
                None
            }
        }
    } else {
        None
    };
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    ensure_row(&mut tx, &source, &apply.path, &file.key).await?;
    let removed=sqlx::query("DELETE FROM metadata_reviews WHERE id=$1 AND owner_id=$2 AND source_id=$3 AND file_key=$4 AND expires_at>now()").bind(apply.review_token).bind(identity.user.id).bind(&source).bind(&file.key).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    if removed.rows_affected() != 1 {
        return Err(crate::bad_request(
            "Preview this match again before applying it.",
        ));
    }
    let before: Option<String> =
        sqlx::query_scalar("SELECT matched_data FROM media_metadata WHERE file_key=$1")
            .bind(&file.key)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    let artwork = store_art(&mut tx, artwork).await?;
    sqlx::query("UPDATE media_metadata SET matched_data=$2,provider=$3,provider_id=$4,matched_artwork=$5,custom_data=CASE WHEN $6 THEN NULL ELSE custom_data END,updated_at=now() WHERE file_key=$1").bind(&file.key).bind(&review.data).bind(review.provider).bind(review.provider_id).bind(artwork).bind(apply.replace_custom).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    audit(
        &mut tx,
        &source,
        &file.key,
        identity.user.id,
        "reviewed_match_applied",
        before,
        Some(review.data),
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(ExtractionResponse {
        metadata: current_view(&state, &identity.user, &source, &apply.path, &file.key).await?,
        warnings,
    }))
}
pub async fn artwork(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(key): Path<String>,
) -> Result<Response, AppError> {
    let record = load(&state, &key).await?.ok_or_else(not_found)?;
    reference(
        &state,
        &identity.user,
        &record.source_id,
        &record.relative_path,
        false,
    )
    .await?;
    let id = art_id(&record).ok_or_else(not_found)?;
    let bytes: Vec<u8> = sqlx::query_scalar("SELECT jpeg FROM metadata_artwork WHERE id=$1")
        .bind(id)
        .fetch_one(&state.db)
        .await
        .map_err(|_| not_found())?;
    Ok((
        [
            ("content-type", "image/jpeg"),
            ("cache-control", "private, no-store"),
            ("x-content-type-options", "nosniff"),
        ],
        bytes,
    )
        .into_response())
}

#[derive(Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct Scan {
    force: bool,
}
#[derive(Serialize, FromRow)]
pub struct Job {
    id: Uuid,
    status: String,
    total: i32,
    processed: i32,
    skipped: i32,
    warnings: i32,
    message: Option<String>,
    stale: bool,
}
pub async fn job(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
) -> Result<Json<Option<Job>>, AppError> {
    let owned: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE id=$1 AND owner_id=$2)")
            .bind(&source)
            .bind(identity.user.id)
            .fetch_one(&state.db)
            .await
            .map_err(|_| auth::unavailable())?;
    if !owned {
        return Err(not_found());
    }
    Ok(Json(sqlx::query_as::<_,Job>("SELECT id,status,total,processed,skipped,warnings,message,(status='running' AND updated_at<now()-interval '5 minutes') AS stale FROM metadata_jobs WHERE source_id=$1 ORDER BY created_at DESC LIMIT 1").bind(source).fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?))
}
pub async fn scan(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(scan): Json<Scan>,
) -> Result<impl IntoResponse, AppError> {
    let permit = state
        .metadata
        .jobs
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::TOO_MANY_REQUESTS,
                "A metadata scan is already running on this API instance.".into(),
            )
        })?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    sqlx::query("UPDATE metadata_jobs SET status='interrupted',message='Previous scan stopped responding.' WHERE source_id=$1 AND status='running' AND updated_at<now()-interval '5 minutes'").bind(&source).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    let id = Uuid::new_v4();
    let result = sqlx::query(
        "INSERT INTO metadata_jobs(id,source_id,owner_id,status) VALUES ($1,$2,$3,'running')",
    )
    .bind(id)
    .bind(&source)
    .bind(identity.user.id)
    .execute(&mut *tx)
    .await;
    if let Err(error) = result {
        if error
            .as_database_error()
            .is_some_and(|error| error.is_unique_violation())
        {
            return Err(AppError(
                StatusCode::CONFLICT,
                "A metadata scan is already running for this source.".into(),
            ));
        }
        return Err(auth::unavailable());
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    tokio::spawn(async move {
        let _permit = permit;
        let result = run_scan(&state, &identity.user, &source, id, scan.force).await;
        if result.is_err() {
            let _=sqlx::query("UPDATE metadata_jobs SET status='failed',message='Scan failed or source/account access changed.',updated_at=now() WHERE id=$1").bind(id).execute(&state.db).await;
        }
    });
    Ok((StatusCode::ACCEPTED, Json(serde_json::json!({"id":id}))))
}
async fn run_scan(
    state: &AppState,
    user: &auth::User,
    source: &str,
    id: Uuid,
    force: bool,
) -> Result<(), AppError> {
    let root = crate::media_root(state, user, source).await?;
    if std::fs::read_dir(&root).is_err() {
        return Err(not_found());
    }
    let files = tokio::task::spawn_blocking(move || crate::scan_directory(&root))
        .await
        .map_err(|_| auth::unavailable())?
        .map_err(|_| auth::unavailable())?;
    sqlx::query("UPDATE metadata_jobs SET total=$2,updated_at=now() WHERE id=$1")
        .bind(id)
        .bind(files.len() as i32)
        .execute(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    let mut skipped = 0;
    let mut warnings = 0;
    for (index, file) in files.into_iter().enumerate() {
        let active:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources s JOIN users u ON u.id=s.owner_id WHERE s.id=$1 AND s.owner_id=$2 AND NOT u.disabled)").bind(source).bind(user.id).fetch_one(&state.db).await.map_err(|_|auth::unavailable())?;
        if !active {
            return Err(not_found());
        }
        let _permit = state
            .metadata
            .work
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| auth::unavailable())?;
        match extract_one(state, user, source, &file.relative_path, force).await {
            Ok(None) => skipped += 1,
            Ok(Some(notes)) => warnings += notes.len() as i32,
            Err(_) => warnings += 1,
        }
        sqlx::query("UPDATE metadata_jobs SET processed=$2,skipped=$3,warnings=$4,updated_at=now() WHERE id=$1").bind(id).bind(index as i32+1).bind(skipped).bind(warnings).execute(&state.db).await.map_err(|_|auth::unavailable())?;
    }
    sqlx::query("UPDATE metadata_jobs SET status='completed',updated_at=now() WHERE id=$1")
        .bind(id)
        .execute(&state.db)
        .await
        .map_err(|_| auth::unavailable())?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn custom_metadata_survives_gathering_and_genres_normalize() {
        let custom = Fields {
            title: Some("My title".into()),
            genres: Some(vec![" Science   Fiction ".into(), "science fiction".into()]),
            ..Fields::default()
        }
        .normalize()
        .unwrap_or_else(|_| panic!("fields"));
        assert_eq!(custom.genres, Some(vec!["science fiction".into()]));
        let local = Fields {
            title: Some("Embedded".into()),
            ..Fields::default()
        };
        let matched = Fields {
            title: Some("Provider".into()),
            ..Fields::default()
        };
        assert_eq!(
            model::effective("file.mp3", &local, &matched, Some(&custom))
                .title
                .as_deref(),
            Some("My title")
        );
        assert_eq!(
            model::effective("file.mp3", &local, &matched, None)
                .title
                .as_deref(),
            Some("Provider")
        );
    }
}
