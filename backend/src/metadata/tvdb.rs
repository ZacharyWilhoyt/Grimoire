//! TheTVDB requests use fixed HTTPS endpoints and never receive file paths.
use super::{
    providers::{self, Art, Candidate, Match, Provider},
    *,
};
use crate::unified_catalog;
use serde_json::{json, Value};
use std::time::Instant;

const BASE: &str = "https://api4.thetvdb.com/v4";
const MAX_RESPONSE: usize = 2 * 1024 * 1024;

pub(super) struct Runtime {
    key: Option<String>,
    token: tokio::sync::Mutex<Option<(String, Instant)>>,
}
impl Runtime {
    pub(super) fn from_env() -> Self {
        let credential = |key| {
            std::env::var(key)
                .ok()
                .filter(|value| !value.trim().is_empty())
        };
        Self {
            key: credential("GRIMOIRE_TVDB_API_KEY"),
            token: Default::default(),
        }
    }
    pub(super) fn enabled(&self) -> bool {
        self.key.is_some()
    }
}

fn unavailable() -> AppError {
    AppError(
        StatusCode::BAD_GATEWAY,
        "TheTVDB is unavailable or rejected the server credentials.".into(),
    )
}

async fn bounded(mut response: reqwest::Response) -> Result<Value, AppError> {
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|size| size > MAX_RESPONSE as u64)
    {
        return Err(unavailable());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| unavailable())? {
        if bytes.len() + chunk.len() > MAX_RESPONSE {
            return Err(unavailable());
        }
        bytes.extend_from_slice(&chunk);
    }
    serde_json::from_slice(&bytes).map_err(|_| unavailable())
}

async fn token(state: &AppState, stale: Option<&str>) -> Result<String, AppError> {
    let mut slot = state.metadata.tvdb.token.lock().await;
    if let Some((token, _)) = slot
        .as_ref()
        .filter(|(token, expires)| stale != Some(token) && *expires > Instant::now())
    {
        return Ok(token.clone());
    }
    let config = &state.metadata.tvdb;
    let key = config.key.as_ref().ok_or_else(unavailable)?;
    let body = json!({"apikey":key});
    providers::budget(state).await?;
    let response = state
        .metadata
        .client
        .post(format!("{BASE}/login"))
        .json(&body)
        .send()
        .await
        .map_err(|_| unavailable())?;
    let data = bounded(response).await?;
    let token = data
        .pointer("/data/token")
        .and_then(Value::as_str)
        .filter(|value| !value.is_empty() && value.len() < 8192)
        .ok_or_else(unavailable)?
        .to_owned();
    *slot = Some((
        token.clone(),
        Instant::now() + Duration::from_secs(24 * 3600),
    ));
    Ok(token)
}

pub(super) async fn get(
    state: &AppState,
    path: &str,
    query: &[(&str, &str)],
) -> Result<Value, AppError> {
    let mut token_value = token(state, None).await?;
    for retry in 0..=1 {
        providers::budget(state).await?;
        let response = state
            .metadata
            .client
            .get(format!("{BASE}{path}"))
            .query(query)
            .bearer_auth(&token_value)
            .send()
            .await
            .map_err(|_| unavailable())?;
        if response.status() == StatusCode::UNAUTHORIZED && retry == 0 {
            token_value = token(state, Some(&token_value)).await?;
            continue;
        }
        if response.status() == StatusCode::NOT_FOUND {
            return Err(crate::bad_request(
                "TheTVDB could not find that series or episode.",
            ));
        }
        return bounded(response).await;
    }
    Err(unavailable())
}

#[derive(Clone, Copy, Default, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Order {
    #[default]
    Official,
    Dvd,
    Absolute,
}
impl Order {
    fn name(self) -> &'static str {
        match self {
            Self::Official => "official",
            Self::Dvd => "dvd",
            Self::Absolute => "absolute",
        }
    }
}

pub(super) fn set_position(context: &mut Fields, input: &Preview) {
    context.season = input.season.or(context.season);
    context.episode = input.episode.or(context.episode);
}

pub(super) async fn lookup(
    state: &AppState,
    id: &str,
    context: &Fields,
    order: Order,
) -> Result<(Match, String), AppError> {
    if !Provider::Tvdb.validate_id(id) {
        return Err(crate::bad_request("Invalid TheTVDB series identifier."));
    }
    let (Some(season), Some(episode)) = (
        context.season.filter(|v| *v <= 100_000),
        context.episode.filter(|v| (1..=100_000).contains(v)),
    ) else {
        return Err(crate::bad_request(
            "Enter a season and episode number before previewing.",
        ));
    };
    let address = format!("/series/{id}/episodes/{}", order.name());
    let value = get(
        state,
        &address,
        &[
            ("season", &season.to_string()),
            ("episodeNumber", &episode.to_string()),
            ("page", "0"),
        ],
    )
    .await?;
    let (mut matched, provider_id) = episode_match(&value, id, season, episode, order)?;
    let language = value
        .pointer("/data/series/originalLanguage")
        .and_then(Value::as_str);
    if language.is_some_and(|language| language != "eng") {
        let series_path = format!("/series/{id}/translations/eng");
        let episode_id = provider_id.split(':').nth(1).ok_or_else(unavailable)?;
        let episode_path = format!("/episodes/{episode_id}/translations/eng");
        let (series_translation, episode_translation) = tokio::join!(
            get(state, &series_path, &[]),
            get(state, &episode_path, &[])
        );
        if let Ok(series) = series_translation {
            if let Some(name) = series
                .get("data")
                .and_then(|value| providers::string(value, "name", 300))
            {
                matched.fields.collection = Some(name);
            }
        }
        if let Ok(episode) = episode_translation {
            if let Some(data) = episode.get("data") {
                matched.fields.title =
                    providers::string(data, "name", 300).or(matched.fields.title);
                matched.fields.description =
                    providers::string(data, "overview", 4000).or(matched.fields.description);
            }
        }
    }
    Ok((matched, provider_id))
}

fn episode_match(
    value: &Value,
    id: &str,
    season: u32,
    episode: u32,
    order: Order,
) -> Result<(Match, String), AppError> {
    let expected = id.parse::<u64>().map_err(|_| unavailable())?;
    let series = value
        .pointer("/data/series")
        .filter(|series| series.get("id").and_then(Value::as_u64) == Some(expected))
        .ok_or_else(unavailable)?;
    let episodes = value
        .pointer("/data/episodes")
        .and_then(Value::as_array)
        .ok_or_else(unavailable)?;
    let [entry] = episodes.as_slice() else {
        return Err(crate::bad_request(
            "TheTVDB did not return a unique episode for that order and number.",
        ));
    };
    if entry
        .get("seriesId")
        .and_then(Value::as_u64)
        .is_some_and(|id| id != expected)
        || matches!(order, Order::Official)
            && (entry.get("seasonNumber").and_then(Value::as_u64) != Some(u64::from(season))
                || entry.get("number").and_then(Value::as_u64) != Some(u64::from(episode)))
    {
        return Err(unavailable());
    }
    let episode_id = entry
        .get("id")
        .and_then(Value::as_u64)
        .filter(|id| *id > 0)
        .ok_or_else(unavailable)?;
    let collection = providers::string(series, "name", 300).ok_or_else(unavailable)?;
    let title = providers::string(entry, "name", 300)
        .ok_or_else(|| crate::bad_request("TheTVDB has no title for this episode."))?;
    let art = series
        .get("image")
        .and_then(Value::as_str)
        .and_then(artwork_path)
        .map(Art::Tvdb);
    let fields = Fields {
        title: Some(title),
        collection: Some(collection),
        season: Some(season),
        episode: Some(episode),
        description: providers::string(entry, "overview", 4000),
        year: providers::string(entry, "aired", 20)
            .as_deref()
            .and_then(providers::year),
        ..Fields::default()
    }
    .normalize()?;
    Ok((
        Match { fields, art },
        format!("{expected}:{episode_id}:{}", order.name()),
    ))
}

fn artwork_path(value: &str) -> Option<String> {
    let url = reqwest::Url::parse(value).ok()?;
    (url.scheme() == "https"
        && url.host_str() == Some("artworks.thetvdb.com")
        && url.port().is_none()
        && url.username().is_empty()
        && url.password().is_none()
        && url.query().is_none()
        && url.fragment().is_none()
        && url.path().starts_with("/banners/")
        && url.path().len() < 500)
        .then(|| url.path().to_owned())
}

pub(crate) fn routes() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/api/catalog/v1/remote-metadata/{id}/search",
            axum::routing::post(remote_search),
        )
        .route(
            "/api/catalog/v1/remote-metadata/{id}/preview",
            axum::routing::post(remote_preview),
        )
        .route(
            "/api/catalog/v1/remote-metadata/{id}/apply",
            axum::routing::post(remote_apply),
        )
        .layer(axum::extract::DefaultBodyLimit::max(8192))
}

async fn remote_search(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    Json(input): Json<Search>,
) -> Result<Json<Vec<Candidate>>, AppError> {
    let item = unified_catalog::owner_item(&state, identity.user.id, id).await?;
    if input.path != item.file.relative_path || input.provider != Provider::Tvdb {
        return Err(unified_catalog::missing());
    }
    let query = input.query.trim();
    if query.is_empty() || query.chars().count() > 200 {
        return Err(crate::bad_request("Enter a search of 1–200 characters."));
    }
    let _permit = online_guard(&state, identity.user.id, Provider::Tvdb, item.file.kind).await?;
    let candidates = providers::search(&state, Provider::Tvdb, query).await?;
    unified_catalog::owner_item(&state, identity.user.id, id).await?;
    Ok(Json(candidates))
}

async fn remote_preview(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    Json(input): Json<Preview>,
) -> Result<Json<Review>, AppError> {
    let user = identity.user.id;
    let item = unified_catalog::owner_item(&state, user, id).await?;
    if input.path != item.file.relative_path || input.provider != Provider::Tvdb {
        return Err(unified_catalog::missing());
    }
    let _permit = online_guard(&state, user, Provider::Tvdb, item.file.kind).await?;
    let mut context = item
        .metadata
        .map(|metadata| metadata.fields)
        .unwrap_or_default();
    let parsed = local::filename_fields(&item.file.name);
    context.season = context.season.or(parsed.season);
    context.episode = context.episode.or(parsed.episode);
    set_position(&mut context, &input);
    let (matched, provider_id) =
        lookup(&state, &input.id, &context, input.order.unwrap_or_default()).await?;
    let review_token = Uuid::new_v4();
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    sqlx::query("DELETE FROM metadata_reviews WHERE expires_at<now()")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let sql=format!("INSERT INTO metadata_reviews(id,owner_id,source_id,file_key,provider,provider_id,data,artwork_ref)
        SELECT $3,$1,$4,$5,'tvdb',$6,$7,$8 FROM ({}) eligible",unified_catalog::OWNER_CHECK);
    let inserted = sqlx::query(&sql)
        .bind(user)
        .bind(id)
        .bind(review_token)
        .bind(&item.source_id)
        .bind(&item.file_key)
        .bind(&provider_id)
        .bind(serde_json::to_string(&matched.fields).map_err(|_| auth::unavailable())?)
        .bind(
            matched
                .art
                .as_ref()
                .map(serde_json::to_string)
                .transpose()
                .map_err(|_| auth::unavailable())?,
        )
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    if inserted.rows_affected() != 1 {
        return Err(unified_catalog::missing());
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(Review {
        review_token,
        provider: Provider::Tvdb,
        provider_id,
        fields: matched.fields,
        has_artwork: matched.art.is_some(),
    }))
}

async fn remote_apply(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(id): Path<Uuid>,
    Json(input): Json<Apply>,
) -> Result<Json<ExtractionResponse>, AppError> {
    let user = identity.user.id;
    let item = unified_catalog::owner_item(&state, user, id).await?;
    if input.path != item.file.relative_path {
        return Err(unified_catalog::missing());
    }
    let _permit = online_guard(&state, user, Provider::Tvdb, item.file.kind).await?;
    let expired = || {
        crate::bad_request("This review expired or belongs to a different file. Preview it again.")
    };
    let row: (String, String, Option<String>) = sqlx::query_as(
        "SELECT provider_id,data,artwork_ref FROM metadata_reviews WHERE id=$1 AND owner_id=$2
        AND source_id=$3 AND file_key=$4 AND provider='tvdb' AND expires_at>now()",
    )
    .bind(input.review_token)
    .bind(user)
    .bind(&item.source_id)
    .bind(&item.file_key)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| auth::unavailable())?
    .ok_or_else(expired)?;
    let mut warnings = Vec::new();
    let artwork = if let Some(art) = row
        .2
        .as_deref()
        .and_then(|art| serde_json::from_str::<Art>(art).ok())
    {
        match providers::artwork(&state, &art).await {
            Ok(bytes) => Some(bytes),
            Err(_) => {
                warnings.push("TheTVDB poster could not be loaded.".into());
                None
            }
        }
    } else {
        None
    };
    let parts: Vec<_> = row.0.split(':').collect();
    let [series, _, order] = parts.as_slice() else {
        return Err(auth::unavailable());
    };
    let group = auth::digest(&format!("tvdb:series:{series}:{order}"));
    let fields: Value = serde_json::from_str(&row.1).map_err(|_| auth::unavailable())?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    let removed = sqlx::query(
        "DELETE FROM metadata_reviews WHERE id=$1 AND owner_id=$2 AND source_id=$3
        AND file_key=$4 AND provider='tvdb' AND expires_at>now()",
    )
    .bind(input.review_token)
    .bind(user)
    .bind(&item.source_id)
    .bind(&item.file_key)
    .execute(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if removed.rows_affected() != 1 {
        return Err(expired());
    }
    let artwork = store_art(&mut tx, artwork).await?;
    let sql=format!("INSERT INTO catalog_remote_tvdb_matches(representation_id,provider_id,group_id,fields,artwork)
        SELECT eligible.representation_id,$3,$4,$5,$6 FROM ({}) eligible
        ON CONFLICT(representation_id) DO UPDATE SET provider_id=excluded.provider_id,group_id=excluded.group_id,
        fields=excluded.fields,artwork=excluded.artwork",unified_catalog::OWNER_CHECK);
    if sqlx::query(&sql)
        .bind(user)
        .bind(id)
        .bind(row.0)
        .bind(group)
        .bind(fields)
        .bind(artwork)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?
        .rows_affected()
        != 1
    {
        return Err(unified_catalog::missing());
    }
    if input.replace_custom {
        sqlx::query("DELETE FROM catalog_remote_metadata_overrides WHERE representation_id=$1")
            .bind(id)
            .execute(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    }
    tx.commit().await.map_err(|_| auth::unavailable())?;
    let metadata = unified_catalog::owner_item(&state, user, id)
        .await?
        .metadata
        .ok_or_else(unified_catalog::missing)?;
    Ok(Json(ExtractionResponse { metadata, warnings }))
}

#[cfg(test)]
pub(crate) fn enable_fixture(runtime: &mut super::Runtime) {
    runtime.tvdb.key = Some("fixture-only".into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn episode_identity_keeps_series_episode_and_selected_order_separate() {
        let fixture = json!({"data":{"series":{"id":42,"name":"Series","image":"https://artworks.thetvdb.com/banners/poster.jpg"},
            "episodes":[{"id":98,"seriesId":42,"name":"Episode","seasonNumber":2,"number":3}]}});
        let (matched, id) = episode_match(&fixture, "42", 2, 3, Order::Official).unwrap();
        assert_eq!(id, "42:98:official");
        assert_eq!(matched.fields.episode, Some(3));
        assert!(matches!(matched.art, Some(Art::Tvdb(_))));
        assert!(episode_match(&fixture, "42", 2, 4, Order::Official).is_err());
        assert!(episode_match(&fixture, "43", 2, 3, Order::Official).is_err());
        let (_, id) = episode_match(&fixture, "42", 1, 14, Order::Absolute).unwrap();
        assert_eq!(id, "42:98:absolute");
        assert!(artwork_path("https://artworks.thetvdb.com.example.com/banners/a.jpg").is_none());
    }
}
