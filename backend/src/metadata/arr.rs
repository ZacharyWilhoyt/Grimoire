//! Optional owner-managed Arr adapters. Only fixed read endpoints are requested.
use super::{
    model::{self, Fields},
    *,
};
use chacha20poly1305::{
    aead::{Aead, KeyInit, Payload},
    XChaCha20Poly1305, XNonce,
};
use rand::RngCore;
use serde_json::{json, Value};

pub struct Config {
    key: Option<[u8; 32]>,
    allowed: HashSet<String>,
    client: reqwest::Client,
}
impl Config {
    pub fn from_env() -> Self {
        let key = std::env::var("GRIMOIRE_INTEGRATION_KEY")
            .ok()
            .and_then(|value| hex::decode(value).ok())
            .and_then(|bytes| bytes.try_into().ok());
        let allowed: Vec<String> = std::env::var("GRIMOIRE_ARR_ALLOWED_URLS")
            .ok()
            .and_then(|value| serde_json::from_str(&value).ok())
            .unwrap_or_default();
        Self::new(key, allowed)
    }
    fn new(key: Option<[u8; 32]>, allowed: Vec<String>) -> Self {
        Self {
            key,
            allowed: allowed
                .into_iter()
                .filter_map(|url| base_url(&url).ok())
                .collect(),
            client: reqwest::Client::builder()
                .redirect(reqwest::redirect::Policy::none())
                .no_proxy()
                .connect_timeout(Duration::from_secs(5))
                .timeout(Duration::from_secs(15))
                .user_agent("Grimoire/0.1")
                .build()
                .expect("Arr HTTP configuration"),
        }
    }
    #[cfg(test)]
    pub fn testing(urls: Vec<String>) -> Self {
        Self::new(Some([17; 32]), urls)
    }
    fn allows(&self, url: &str) -> bool {
        self.key.is_some() && self.allowed.contains(url)
    }
}
fn base_url(value: &str) -> Result<String, AppError> {
    let value = value.trim();
    if value.len() > 2048 || value.contains(['\\', '%']) {
        return Err(crate::bad_request(
            "Use a plain Arr base URL without encoded path components.",
        ));
    }
    let mut url = reqwest::Url::parse(value)
        .map_err(|_| crate::bad_request("Enter an absolute http:// or https:// Arr URL."))?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(crate::bad_request(
            "Arr URLs cannot contain credentials, queries, or fragments.",
        ));
    }
    url.set_path(&format!("{}/", url.path().trim_end_matches('/')));
    Ok(url.to_string())
}
#[derive(Clone, Copy, Deserialize, Serialize, PartialEq, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Sonarr,
    Radarr,
    Lidarr,
}
impl Kind {
    fn name(self) -> &'static str {
        match self {
            Self::Sonarr => "sonarr",
            Self::Radarr => "radarr",
            Self::Lidarr => "lidarr",
        }
    }
    fn supports(self, kind: &str) -> bool {
        matches!(
            (self, kind),
            (Self::Sonarr | Self::Radarr, "video") | (Self::Lidarr, "music")
        )
    }
}
#[derive(Clone, FromRow)]
struct Connection {
    id: Uuid,
    source_id: String,
    name: String,
    kind: String,
    base_url: String,
    remote_root: String,
    source_subpath: String,
    api_key_encrypted: Vec<u8>,
    enabled: bool,
    revision: Uuid,
}
impl Connection {
    fn kind(&self) -> Result<Kind, AppError> {
        serde_json::from_value(json!(self.kind)).map_err(|_| auth::unavailable())
    }
    fn aad(&self) -> String {
        format!(
            "{}\0{}\0{}\0{}",
            self.source_id, self.id, self.kind, self.base_url
        )
    }
    fn seal(&mut self, config: &Config, key: &str) -> Result<(), AppError> {
        let master = config.key.as_ref().ok_or_else(not_configured)?;
        let mut nonce = [0u8; 24];
        rand::rngs::OsRng.fill_bytes(&mut nonce);
        let ciphertext = XChaCha20Poly1305::new(master.into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: key.as_bytes(),
                    aad: self.aad().as_bytes(),
                },
            )
            .map_err(|_| auth::unavailable())?;
        self.api_key_encrypted = nonce.into_iter().chain(ciphertext).collect();
        Ok(())
    }
    fn secret(&self, config: &Config) -> Result<String, AppError> {
        if !config.allows(&self.base_url) {
            return Err(not_configured());
        }
        if self.api_key_encrypted.len() < 40 {
            return Err(not_configured());
        }
        let master = config.key.as_ref().ok_or_else(not_configured)?;
        let bytes = XChaCha20Poly1305::new(master.into())
            .decrypt(
                XNonce::from_slice(&self.api_key_encrypted[..24]),
                Payload {
                    msg: &self.api_key_encrypted[24..],
                    aad: self.aad().as_bytes(),
                },
            )
            .map_err(|_| not_configured())?;
        String::from_utf8(bytes).map_err(|_| not_configured())
    }
    fn summary(&self, config: &Config) -> Value {
        json!({"id":self.id,"name":self.name,"kind":self.kind,"base_url":self.base_url,"remote_root":self.remote_root,"source_subpath":self.source_subpath,"enabled":self.enabled,"has_api_key":true,"usable":self.enabled && config.allows(&self.base_url)})
    }
}
fn not_configured() -> AppError {
    AppError(StatusCode::SERVICE_UNAVAILABLE,"Arr integration needs a server encryption key and an operator-approved base URL. Check the integration settings documentation.".into())
}
fn remote_error() -> AppError {
    AppError(StatusCode::BAD_GATEWAY,"Arr could not be read. Check the connection URL, API key, version, and service availability.".into())
}
async fn owned(state: &AppState, source: &str, user: Uuid) -> Result<(), AppError> {
    let owned: bool =
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM sources WHERE id=$1 AND owner_id=$2)")
            .bind(source)
            .bind(user)
            .fetch_one(&state.db)
            .await
            .map_err(|_| auth::unavailable())?;
    if owned {
        Ok(())
    } else {
        Err(not_found())
    }
}
async fn load_connection(state: &AppState, source: &str, id: Uuid) -> Result<Connection, AppError> {
    sqlx::query_as::<_, Connection>("SELECT * FROM arr_connections WHERE source_id=$1 AND id=$2")
        .bind(source)
        .bind(id)
        .fetch_optional(&state.db)
        .await
        .map_err(|_| auth::unavailable())?
        .ok_or_else(not_found)
}
pub async fn list(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
) -> Result<Json<Value>, AppError> {
    owned(&state, &source, identity.user.id).await?;
    let config = &state.metadata.arr;
    let rows = sqlx::query_as::<_, Connection>(
        "SELECT * FROM arr_connections WHERE source_id=$1 ORDER BY name,id",
    )
    .bind(&source)
    .fetch_all(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    Ok(Json(
        json!({"configured":config.key.is_some() && !config.allowed.is_empty(),"online":state.metadata.online,"connections":rows.iter().map(|row|row.summary(config)).collect::<Vec<_>>()}),
    ))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    name: String,
    kind: Kind,
    base_url: String,
    remote_root: String,
    #[serde(default)]
    source_subpath: String,
    api_key: Option<String>,
    #[serde(default = "yes")]
    enabled: bool,
}
fn yes() -> bool {
    true
}
fn components(path: &str) -> Result<Vec<String>, AppError> {
    if path.len() > 4096 || path.contains('\0') {
        return Err(crate::bad_request("Invalid mapped folder."));
    }
    let path = path.replace('\\', "/");
    let parts: Vec<_> = path
        .split('/')
        .filter(|part| !part.is_empty())
        .map(str::to_string)
        .collect();
    if parts.iter().any(|part| part == "." || part == "..") {
        return Err(crate::bad_request(
            "Mapped folders cannot contain . or .. components.",
        ));
    }
    Ok(parts)
}
fn root(path: &str) -> Result<String, AppError> {
    let path = path.trim().replace('\\', "/");
    if !(path.starts_with('/')
        || path.as_bytes().get(1) == Some(&b':')
            && path.as_bytes().get(2) == Some(&b'/')
            && path.as_bytes()[0].is_ascii_alphabetic())
    {
        return Err(crate::bad_request(
            "The Arr media folder must be an absolute path.",
        ));
    }
    let parts = components(&path)?;
    Ok(format!(
        "{}{}",
        if path.starts_with('/') { "/" } else { "" },
        parts.join("/")
    ))
}
fn subpath(path: &str) -> Result<String, AppError> {
    if path.starts_with(['/', '\\']) || path.contains(':') {
        return Err(crate::bad_request(
            "Use a relative folder inside this Grimoire source.",
        ));
    }
    Ok(components(path)?.join("/"))
}
fn target(connection: &Connection, path: &str) -> Result<String, AppError> {
    let path = components(path)?.join("/");
    let tail = if connection.source_subpath.is_empty() {
        path.as_str()
    } else {
        path.strip_prefix(&format!("{}/", connection.source_subpath))
            .ok_or_else(|| {
                crate::bad_request("This file is outside the connection's mapped folder.")
            })?
    };
    Ok(format!(
        "{}/{}",
        connection.remote_root.trim_end_matches('/'),
        tail
    ))
}
pub async fn save(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, Uuid)>,
    Json(settings): Json<Settings>,
) -> Result<Json<Value>, AppError> {
    owned(&state, &source, identity.user.id).await?;
    let base_url = base_url(&settings.base_url)?;
    if !state.metadata.arr.allows(&base_url) {
        return Err(not_configured());
    }
    let name = model::text(&settings.name, 80);
    if name.is_empty() || settings.name.chars().count() > 80 {
        return Err(crate::bad_request(
            "Use a connection name of 1–80 characters.",
        ));
    }
    let previous = sqlx::query_as::<_, Connection>(
        "SELECT * FROM arr_connections WHERE source_id=$1 AND id=$2",
    )
    .bind(&source)
    .bind(id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| auth::unavailable())?;
    let key = match settings.api_key.filter(|key| !key.is_empty()) {
        Some(key) => key,
        None => {
            let old = previous.as_ref().ok_or_else(|| {
                crate::bad_request("An API key is required for a new connection.")
            })?;
            if old.base_url != base_url || old.kind != settings.kind.name() {
                return Err(crate::bad_request(
                    "Re-enter the API key when changing the app or URL.",
                ));
            }
            old.secret(&state.metadata.arr)?
        }
    };
    if key.len() > 256
        || key.len() < 8
        || !key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"-_".contains(&byte))
    {
        return Err(crate::bad_request("Enter a valid Arr API key."));
    }
    let mut row = Connection {
        id,
        source_id: source.clone(),
        name,
        kind: settings.kind.name().into(),
        base_url,
        remote_root: root(&settings.remote_root)?,
        source_subpath: subpath(&settings.source_subpath)?,
        api_key_encrypted: vec![],
        enabled: settings.enabled,
        revision: Uuid::new_v4(),
    };
    row.seal(&state.metadata.arr, &key)?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM arr_connections WHERE source_id=$1 AND id<>$2")
            .bind(&source)
            .bind(id)
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    if count >= 12 {
        return Err(crate::bad_request(
            "Use at most 12 Arr connections per source.",
        ));
    }
    let saved = sqlx::query("INSERT INTO arr_connections(id,source_id,name,kind,base_url,remote_root,source_subpath,api_key_encrypted,enabled,revision) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(id) DO UPDATE SET name=$3,kind=$4,base_url=$5,remote_root=$6,source_subpath=$7,api_key_encrypted=$8,enabled=$9,revision=$10,updated_at=now() WHERE arr_connections.source_id=$2").bind(id).bind(&source).bind(&row.name).bind(&row.kind).bind(&row.base_url).bind(&row.remote_root).bind(&row.source_subpath).bind(&row.api_key_encrypted).bind(row.enabled).bind(row.revision).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    if saved.rows_affected() != 1 {
        return Err(not_found());
    }
    // Audit deliberately excludes the API key, ciphertext, and private service URL.
    audit(
        &mut tx,
        &source,
        &id.to_string(),
        identity.user.id,
        "arr_connection_saved",
        None,
        Some(json!({"kind":row.kind,"enabled":row.enabled,"revision":row.revision}).to_string()),
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(row.summary(&state.metadata.arr)))
}
pub async fn remove(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, Uuid)>,
) -> Result<StatusCode, AppError> {
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    sqlx::query("DELETE FROM arr_connections WHERE source_id=$1 AND id=$2")
        .bind(&source)
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    audit(
        &mut tx,
        &source,
        &id.to_string(),
        identity.user.id,
        "arr_connection_removed",
        None,
        None,
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(StatusCode::NO_CONTENT)
}
async fn guard(
    state: &AppState,
    user: Uuid,
    connection: &Connection,
) -> Result<tokio::sync::OwnedSemaphorePermit, AppError> {
    if !state.metadata.online || !connection.enabled {
        return Err(crate::bad_request(
            "This connection or online metadata access is disabled.",
        ));
    }
    if !state.metadata.arr.allows(&connection.base_url) {
        return Err(not_configured());
    }
    auth::rate_limit(&state.db, format!("arr:user:{user}"), 30, 300).await?;
    state
        .metadata
        .work
        .clone()
        .try_acquire_owned()
        .map_err(|_| {
            AppError(
                StatusCode::TOO_MANY_REQUESTS,
                "Metadata processing is busy.".into(),
            )
        })
}
// Only this adapter constructs paths. Never forward response URLs, redirects,
// browser cookies, or credentials to a second origin.
async fn download(
    state: &AppState,
    row: &Connection,
    path: &str,
    query: &[(&str, String)],
    limit: usize,
) -> Result<Vec<u8>, AppError> {
    let key = row.secret(&state.metadata.arr)?;
    let mut url = reqwest::Url::parse(&row.base_url)
        .map_err(|_| not_configured())?
        .join(path)
        .map_err(|_| remote_error())?;
    if !url.as_str().starts_with(&row.base_url) {
        return Err(remote_error());
    }
    url.query_pairs_mut()
        .extend_pairs(query.iter().map(|(k, v)| (*k, v.as_str())));
    let mut response = state
        .metadata
        .arr
        .client
        .get(url)
        .header("X-Api-Key", key)
        .send()
        .await
        .map_err(|_| remote_error())?;
    if !response.status().is_success()
        || response
            .content_length()
            .is_some_and(|length| length > limit as u64)
    {
        return Err(remote_error());
    }
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| remote_error())? {
        if bytes.len() + chunk.len() > limit {
            return Err(remote_error());
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
async fn get(
    state: &AppState,
    row: &Connection,
    path: &str,
    query: &[(&str, String)],
) -> Result<Value, AppError> {
    serde_json::from_slice(&download(state, row, path, query, 8 * 1024 * 1024).await?)
        .map_err(|_| remote_error())
}
async fn api(state: &AppState, row: &Connection) -> Result<String, AppError> {
    let info = get(state, row, "api", &[]).await?;
    let current = info["current"].as_str().ok_or_else(remote_error)?;
    let supported = match row.kind()? {
        Kind::Sonarr => matches!(current, "v3" | "v5"),
        Kind::Radarr => current == "v3",
        Kind::Lidarr => current == "v1",
    };
    if !supported {
        return Err(AppError(StatusCode::BAD_GATEWAY,"This Arr API version is not supported. Sonarr v3/v5, Radarr v3, and Lidarr v1 APIs are supported.".into()));
    }
    Ok(format!("api/{current}"))
}
pub async fn test(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, Uuid)>,
) -> Result<Json<Value>, AppError> {
    owned(&state, &source, identity.user.id).await?;
    let row = load_connection(&state, &source, id).await?;
    let _permit = guard(&state, identity.user.id, &row).await?;
    let api = api(&state, &row).await?;
    let status = get(&state, &row, &format!("{api}/system/status"), &[]).await?;
    if !status["appName"]
        .as_str()
        .is_some_and(|name| name.eq_ignore_ascii_case(&row.kind))
    {
        return Err(remote_error());
    }
    owned(&state, &source, identity.user.id).await?;
    Ok(Json(
        json!({"ok":true,"app":row.kind,"version":status["version"].as_str().map(|value|model::text(value,80)),"api":api}),
    ))
}
fn array(value: &Value) -> Result<&[Value], AppError> {
    let values = value.as_array().ok_or_else(remote_error)?;
    if values.len() > 20_000 {
        return Err(remote_error());
    }
    Ok(values)
}
fn id(value: &Value) -> Result<i64, AppError> {
    value["id"]
        .as_i64()
        .filter(|id| *id > 0)
        .ok_or_else(remote_error)
}
fn string(value: &Value, key: &str, limit: usize) -> Option<String> {
    value[key]
        .as_str()
        .map(|value| model::text(value, limit))
        .filter(|value| !value.is_empty())
}
fn genres(value: &Value) -> Option<Vec<String>> {
    value["genres"].as_array().map(|genres| {
        genres
            .iter()
            .filter_map(Value::as_str)
            .take(20)
            .map(|value| model::text(value, 60))
            .collect()
    })
}
fn fields(value: &Value) -> Fields {
    Fields {
        title: string(value, "title", 300),
        description: string(value, "overview", 4000),
        genres: genres(value),
        year: value["year"]
            .as_i64()
            .and_then(|year| i32::try_from(year).ok())
            .filter(|year| (1..=9999).contains(year)),
        ..Fields::default()
    }
}
fn same_path(value: &str, target: &str) -> bool {
    root(value).ok().as_deref() == Some(target)
}
fn parent<'a>(values: &'a [Value], target: &str) -> Result<&'a Value, AppError> {
    let matches: Vec<_> = values
        .iter()
        .filter(|value| {
            value["path"]
                .as_str()
                .and_then(|path| root(path).ok())
                .is_some_and(|path| target.starts_with(&format!("{}/", path.trim_end_matches('/'))))
        })
        .collect();
    if matches.len() != 1 {
        return Err(crate::bad_request("The mapped file does not identify exactly one Arr series/artist. Check folder mapping."));
    }
    Ok(matches[0])
}
fn unique(values: Vec<&Value>) -> Result<&Value, AppError> {
    if values.len() != 1 {
        Err(crate::bad_request("Arr has no unique existing file at the mapped path. Check folder mapping and import the file into Arr first."))
    } else {
        Ok(values[0])
    }
}
#[derive(Serialize, Deserialize)]
struct Cover {
    id: i64,
    filename: String,
    entity: String,
    api: String,
}
fn cover(value: &Value, entity: &str, api: &str) -> Option<Cover> {
    let name = if entity == "album" { "cover" } else { "poster" };
    let image = value["images"]
        .as_array()?
        .iter()
        .find(|image| image["coverType"].as_str() == Some(name))?;
    let filename = image["url"]
        .as_str()?
        .split('?')
        .next()?
        .rsplit('/')
        .next()?;
    if ![
        format!("{name}.jpg"),
        format!("{name}.png"),
        format!("{name}.webp"),
    ]
    .contains(&filename.to_string())
    {
        return None;
    }
    Some(Cover {
        id: id(value).ok()?,
        filename: filename.into(),
        entity: entity.into(),
        api: api.into(),
    })
}
async fn matched(
    state: &AppState,
    row: &Connection,
    path: &str,
    api: &str,
) -> Result<(Fields, Option<Cover>, String), AppError> {
    let target = target(row, path)?;
    match row.kind()? {
        Kind::Radarr => {
            let movies = get(state, row, &format!("{api}/movie"), &[]).await?;
            let movie = unique(
                array(&movies)?
                    .iter()
                    .filter(|movie| {
                        let file = &movie["movieFile"];
                        let full = file["path"].as_str().map(str::to_string).or_else(|| {
                            Some(format!(
                                "{}/{}",
                                movie["path"].as_str()?,
                                file["relativePath"].as_str()?
                            ))
                        });
                        full.is_some_and(|path| same_path(&path, &target))
                    })
                    .collect(),
            )?;
            let mut fields = fields(movie);
            fields.collection = string(&movie["collection"], "title", 300);
            Ok((fields, cover(movie, "movie", api), id(movie)?.to_string()))
        }
        Kind::Sonarr => {
            let series = get(state, row, &format!("{api}/series"), &[]).await?;
            let series = parent(array(&series)?, &target)?;
            let series_id = id(series)?;
            let query = [("seriesId", series_id.to_string())];
            let files = get(state, row, &format!("{api}/episodefile"), &query).await?;
            let file = unique(
                array(&files)?
                    .iter()
                    .filter(|file| {
                        let full = file["path"].as_str().map(str::to_string).or_else(|| {
                            Some(format!(
                                "{}/{}",
                                series["path"].as_str()?,
                                file["relativePath"].as_str()?
                            ))
                        });
                        full.is_some_and(|path| same_path(&path, &target))
                    })
                    .collect(),
            )?;
            let file_id = id(file)?;
            let episodes = get(
                state,
                row,
                &format!("{api}/episode"),
                &[
                    ("seriesId", series_id.to_string()),
                    ("episodeFileId", file_id.to_string()),
                ],
            )
            .await?;
            let mut episodes: Vec<_> = array(&episodes)?
                .iter()
                .filter(|episode| episode["episodeFileId"].as_i64() == Some(file_id))
                .collect();
            episodes.sort_by_key(|episode| {
                (
                    episode["seasonNumber"].as_i64(),
                    episode["episodeNumber"].as_i64(),
                )
            });
            let first = episodes
                .first()
                .ok_or_else(|| crate::bad_request("Arr has no episodes linked to this file."))?;
            let mut fields = fields(series);
            fields.collection = string(series, "title", 300);
            fields.title = Some(model::text(
                &episodes
                    .iter()
                    .filter_map(|episode| episode["title"].as_str())
                    .take(20)
                    .collect::<Vec<_>>()
                    .join(" / "),
                300,
            ));
            fields.description = string(first, "overview", 4000);
            fields.season = first["seasonNumber"]
                .as_u64()
                .and_then(|number| u32::try_from(number).ok());
            fields.episode = first["episodeNumber"]
                .as_u64()
                .and_then(|number| u32::try_from(number).ok());
            Ok((
                fields,
                cover(series, "series", api),
                format!("{series_id}:{file_id}"),
            ))
        }
        Kind::Lidarr => {
            let artists = get(state, row, &format!("{api}/artist"), &[]).await?;
            let artist = parent(array(&artists)?, &target)?;
            let artist_id = id(artist)?;
            let files = get(
                state,
                row,
                &format!("{api}/trackfile"),
                &[("artistId", artist_id.to_string())],
            )
            .await?;
            let file = unique(
                array(&files)?
                    .iter()
                    .filter(|file| {
                        file["path"]
                            .as_str()
                            .is_some_and(|path| same_path(path, &target))
                    })
                    .collect(),
            )?;
            let file_id = id(file)?;
            let album_id = file["albumId"]
                .as_i64()
                .filter(|id| *id > 0)
                .ok_or_else(remote_error)?;
            let album = get(state, row, &format!("{api}/album/{album_id}"), &[]).await?;
            let tracks = get(
                state,
                row,
                &format!("{api}/track"),
                &[("albumId", album_id.to_string())],
            )
            .await?;
            let track = unique(
                array(&tracks)?
                    .iter()
                    .filter(|track| track["trackFileId"].as_i64() == Some(file_id))
                    .collect(),
            )?;
            let mut fields = fields(&album);
            fields.title = string(track, "title", 300);
            fields.creator = string(artist, "artistName", 300);
            fields.collection = string(&album, "title", 300);
            fields.genres = fields
                .genres
                .filter(|genres| !genres.is_empty())
                .or_else(|| genres(artist));
            fields.track = track["absoluteTrackNumber"]
                .as_u64()
                .or_else(|| {
                    track["trackNumber"]
                        .as_str()
                        .and_then(|number| number.parse().ok())
                })
                .and_then(|number| u32::try_from(number).ok());
            fields.disc = track["mediumNumber"]
                .as_u64()
                .and_then(|number| u32::try_from(number).ok());
            fields.year = album["releaseDate"]
                .as_str()
                .and_then(|date| date.get(..4))
                .and_then(|year| year.parse().ok())
                .filter(|year| (1..=9999).contains(year));
            Ok((
                fields,
                cover(&album, "album", api),
                format!("{album_id}:{}", id(track)?),
            ))
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewFile {
    path: String,
}
pub async fn preview(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id)): Path<(String, Uuid)>,
    Json(body): Json<PreviewFile>,
) -> Result<Json<Value>, AppError> {
    let file = reference(&state, &identity.user, &source, &body.path, true).await?;
    let row = load_connection(&state, &source, id).await?;
    if !row.kind()?.supports(file.kind) {
        return Err(crate::bad_request(
            "This Arr app does not support this media type.",
        ));
    }
    let _permit = guard(&state, identity.user.id, &row).await?;
    let api = api(&state, &row).await?;
    let (fields, cover, provider_id) = matched(&state, &row, &body.path, &api).await?;
    let fields = fields.normalize()?;
    let token = Uuid::new_v4();
    let provider = format!("arr_{}", row.kind);
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    check_review(&mut tx, &source, id, Some(row.revision)).await?;
    sqlx::query("DELETE FROM metadata_reviews WHERE expires_at<now()")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    sqlx::query("INSERT INTO metadata_reviews(id,owner_id,source_id,file_key,provider,provider_id,data,artwork_ref,arr_connection_id,arr_revision) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)").bind(token).bind(identity.user.id).bind(&source).bind(file.key).bind(&provider).bind(&provider_id).bind(serde_json::to_string(&fields).map_err(|_|auth::unavailable())?).bind(cover.as_ref().map(serde_json::to_string).transpose().map_err(|_|auth::unavailable())?).bind(id).bind(row.revision).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(
        json!({"review_token":token,"provider":provider,"provider_id":provider_id,"fields":fields,"has_artwork":cover.is_some()}),
    ))
}
pub(super) async fn check_review(
    tx: &mut Transaction<'_, Postgres>,
    source: &str,
    id: Uuid,
    revision: Option<Uuid>,
) -> Result<(), AppError> {
    let current: Option<Uuid> = sqlx::query_scalar(
        "SELECT revision FROM arr_connections WHERE source_id=$1 AND id=$2 AND enabled FOR SHARE",
    )
    .bind(source)
    .bind(id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if current.is_none() || current != revision {
        return Err(crate::bad_request(
            "The Arr connection changed or was disabled. Preview the match again.",
        ));
    }
    Ok(())
}
pub(super) async fn review_artwork(
    state: &AppState,
    source: &str,
    id: Uuid,
    revision: Option<Uuid>,
    art: Option<&str>,
    warnings: &mut Vec<String>,
) -> Result<Option<Vec<u8>>, AppError> {
    let row = load_connection(state, source, id).await?;
    if !state.metadata.online || !row.enabled || Some(row.revision) != revision {
        return Err(crate::bad_request(
            "The Arr connection changed or was disabled. Preview the match again.",
        ));
    }
    if !state.metadata.arr.allows(&row.base_url) {
        return Err(not_configured());
    }
    let Some(art) = art else { return Ok(None) };
    let cover: Cover = serde_json::from_str(art).map_err(|_| remote_error())?;
    let expected = match row.kind()? {
        Kind::Sonarr => "series",
        Kind::Radarr => "movie",
        Kind::Lidarr => "album",
    };
    if cover.id <= 0
        || cover.entity != expected
        || !matches!(cover.api.as_str(), "api/v1" | "api/v3" | "api/v5")
        || ![
            "poster.jpg",
            "poster.png",
            "poster.webp",
            "cover.jpg",
            "cover.png",
            "cover.webp",
        ]
        .contains(&cover.filename.as_str())
    {
        return Err(remote_error());
    }
    let path = if expected == "album" {
        format!(
            "{}/mediacover/album/{}/{}",
            cover.api, cover.id, cover.filename
        )
    } else {
        format!("{}/mediacover/{}/{}", cover.api, cover.id, cover.filename)
    };
    let result = async {
        let bytes = download(state, &row, &path, &[], 8 * 1024 * 1024).await?;
        tokio::task::spawn_blocking(move || super::local::jpeg(&bytes))
            .await
            .map_err(|_| remote_error())?
    }
    .await;
    match result {
        Ok(bytes) => Ok(Some(bytes)),
        Err(_) => {
            warnings.push("Arr artwork was unavailable; text metadata was applied and local artwork remains available.".into());
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn connection() -> Connection {
        Connection {
            id: Uuid::new_v4(),
            source_id: "source".into(),
            name: "TV".into(),
            kind: "sonarr".into(),
            base_url: "https://arr.example/sonarr/".into(),
            remote_root: "/tv".into(),
            source_subpath: "shows".into(),
            api_key_encrypted: vec![],
            enabled: true,
            revision: Uuid::new_v4(),
        }
    }
    #[test]
    fn credentials_are_authenticated_to_connection_and_destination() {
        let config = Config::testing(vec![
            "https://arr.example/sonarr/".into(),
            "https://other.example/".into(),
        ]);
        let mut row = connection();
        row.seal(&config, "synthetic-test-key")
            .unwrap_or_else(|_| panic!("encrypt"));
        assert!(!row
            .api_key_encrypted
            .windows(18)
            .any(|window| window == b"synthetic-test-key"));
        assert_eq!(
            row.secret(&config).unwrap_or_default(),
            "synthetic-test-key"
        );
        let encrypted = row.api_key_encrypted.clone();
        row.seal(&config, "synthetic-test-key")
            .unwrap_or_else(|_| panic!("encrypt"));
        assert_ne!(row.api_key_encrypted, encrypted);
        row.base_url = "https://other.example/".into();
        assert!(row.secret(&config).is_err());
        row.base_url = "https://arr.example/sonarr/".into();
        row.source_id = "other-source".into();
        assert!(row.secret(&config).is_err());
    }
    #[test]
    fn mapping_requires_complete_components_and_allowlisted_base() {
        let row = connection();
        assert_eq!(
            target(&row, "shows/Show/episode.mkv").unwrap_or_default(),
            "/tv/Show/episode.mkv"
        );
        assert!(target(&row, "shows-other/episode.mkv").is_err());
        assert!(target(&row, "shows/../secret.mkv").is_err());
        assert!(root("relative/path").is_err());
        assert_eq!(root("D:\\TV\\Show").unwrap_or_default(), "D:/TV/Show");
        assert!(subpath("/absolute").is_err());
        let config = Config::testing(vec!["https://arr.example/sonarr/".into()]);
        assert!(config.allows(&base_url("https://arr.example/sonarr").unwrap_or_default()));
        assert!(!config.allows("https://arr.example/radarr/"));
        assert!(!config.allows("http://169.254.169.254/"));
        for url in [
            "file:///etc/passwd",
            "https://user:secret@arr.example/",
            "https://arr.example/?apikey=secret",
            "https://arr.example/#secret",
            "https://arr.example/%2fadmin",
        ] {
            assert!(base_url(url).is_err(), "{url}");
        }
    }
}
