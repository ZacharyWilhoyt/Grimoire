//! Local Docker management through the operator's private controller socket.
use super::*;

pub struct Client {
    http: reqwest::Client,
}

#[derive(Deserialize, Serialize)]
pub(super) struct Instance {
    pub id: Uuid,
    pub status: String,
    stage: Option<String>,
    error: Option<String>,
    local_port: Option<u16>,
    version: Option<String>,
}

impl Client {
    pub(super) fn from_env() -> Option<Self> {
        let socket = std::env::var_os("GRIMOIRE_ARR_CONTROLLER_SOCKET")?;
        let socket = PathBuf::from(socket);
        if !socket.is_absolute() {
            return None;
        }
        Self::at(socket)
    }

    fn at(socket: PathBuf) -> Option<Self> {
        reqwest::Client::builder()
            .unix_socket(socket)
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .connect_timeout(Duration::from_secs(1))
            .timeout(Duration::from_secs(5))
            .build()
            .ok()
            .map(|http| Self { http })
    }

    async fn request<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        payload: Option<Value>,
    ) -> Result<T, AppError> {
        let url = format!("http://localhost{path}");
        let request = match payload {
            Some(payload) => self.http.post(url).json(&payload),
            None => self.http.get(url),
        };
        let mut response = request.send().await.map_err(|_| offline())?;
        let status = response.status();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| offline())? {
            if bytes.len() + chunk.len() > 64 * 1024 {
                return Err(offline());
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            // The controller emits only fixed messages. Never relay Docker or Arr bodies.
            let error = serde_json::from_slice::<Value>(&bytes)
                .ok()
                .and_then(|value| value["error"].as_str().map(|error| model::text(error, 200)))
                .filter(|error| !error.is_empty())
                .unwrap_or_else(|| "Managed Arr operation failed.".into());
            return Err(crate::bad_request(error));
        }
        serde_json::from_slice(&bytes).map_err(|_| offline())
    }

    pub(super) async fn list(&self, source: &str, ids: &[Uuid]) -> Result<Vec<Instance>, AppError> {
        #[derive(Deserialize)]
        struct List {
            instances: Vec<Instance>,
        }
        let mut params = vec![("source", source.to_string())];
        params.extend(ids.iter().map(|id| ("id", id.to_string())));
        let query = reqwest::Url::parse_with_params("http://localhost/v1/instances", params)
            .map_err(|_| offline())?;
        let list: List = self
            .request(
                &format!("/v1/instances?{}", query.query().unwrap_or_default()),
                None,
            )
            .await?;
        Ok(list.instances)
    }
}

fn offline() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "The managed Arr controller is unavailable. Check the deployment's managed Arr setup."
            .into(),
    )
}

pub(super) fn authorized<'a>(
    state: &'a AppState,
    identity: &auth::Identity,
) -> Result<&'a Client, AppError> {
    if !identity.user.is_admin {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "A source-owning administrator is required to manage local Arr containers.".into(),
        ));
    }
    state.metadata.arr.managed.as_ref().ok_or_else(offline)
}

pub(super) fn base(id: Uuid, kind: Kind) -> String {
    let port = match kind {
        Kind::Sonarr => 8989,
        Kind::Radarr => 7878,
        Kind::Lidarr => 8686,
    };
    format!("http://grimoire-arr-{id}:{port}/")
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Create {
    id: Uuid,
    name: String,
    kind: Kind,
    #[serde(default)]
    source_subpath: String,
    username: String,
    password: String,
    #[serde(default)]
    writable_media: bool,
}

pub async fn create(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path(source): Path<String>,
    Json(input): Json<Create>,
) -> Result<Json<Value>, AppError> {
    owned(&state, &source, identity.user.id).await?;
    let client = authorized(&state, &identity)?;
    if !state.metadata.online {
        return Err(crate::bad_request(
            "Enable online metadata access before provisioning an Arr connection.",
        ));
    }
    auth::rate_limit(
        &state.db,
        format!("arr:manage:{}", identity.user.id),
        30,
        300,
    )
    .await?;
    let name = model::text(&input.name, 80);
    if name.is_empty() || input.name.chars().count() > 80 {
        return Err(crate::bad_request(
            "Use a connection name of 1–80 characters.",
        ));
    }
    if !(3..=64).contains(&input.username.len())
        || !input
            .username
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"_-".contains(&c))
    {
        return Err(crate::bad_request(
            "Use 3–64 letters, digits, underscores or hyphens for the Arr username.",
        ));
    }
    if !(12..=128).contains(&input.password.chars().count())
        || input.password.chars().any(char::is_control)
    {
        return Err(crate::bad_request(
            "Use a 12–128 character Arr password without control characters.",
        ));
    }
    let subpath = subpath(&input.source_subpath)?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    let source_path: String = sqlx::query_scalar("SELECT path FROM sources WHERE id=$1")
        .bind(&source)
        .fetch_one(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let root = tokio::fs::canonicalize(&source_path)
        .await
        .map_err(|_| crate::bad_request("The source directory is unavailable."))?;
    let media = tokio::fs::canonicalize(root.join(&subpath))
        .await
        .map_err(|_| crate::bad_request("The selected source folder is unavailable."))?;
    if !media.starts_with(&root) || !media.is_dir() {
        return Err(crate::bad_request("Choose a directory inside this source."));
    }
    let existing = sqlx::query_as::<_, Connection>("SELECT * FROM arr_connections WHERE id=$1")
        .bind(input.id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    let key = if let Some(row) = existing {
        if row.source_id != source
            || !row.managed
            || row.kind != input.kind.name()
            || row.source_subpath != subpath
            || row.writable_media != input.writable_media
        {
            return Err(crate::bad_request(
                "Retry with the original app, folder and media permissions.",
            ));
        }
        row.secret(&state.metadata.arr)?
    } else {
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM arr_connections WHERE source_id=$1")
                .bind(&source)
                .fetch_one(&mut *tx)
                .await
                .map_err(|_| auth::unavailable())?;
        if count >= 12 {
            return Err(crate::bad_request(
                "Use at most 12 Arr connections per source.",
            ));
        }
        let mut secret = [0; 32];
        rand::rngs::OsRng.fill_bytes(&mut secret);
        let key = hex::encode(secret);
        let mut row = Connection {
            id: input.id,
            source_id: source.clone(),
            name,
            kind: input.kind.name().into(),
            base_url: base(input.id, input.kind),
            remote_root: "/media".into(),
            source_subpath: subpath,
            api_key_encrypted: Vec::new(),
            enabled: true,
            revision: Uuid::new_v4(),
            managed: true,
            writable_media: input.writable_media,
        };
        row.seal(&state.metadata.arr, &key)?;
        sqlx::query("INSERT INTO arr_connections(id,source_id,name,kind,base_url,remote_root,source_subpath,api_key_encrypted,revision,managed,writable_media) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,true,$10)")
            .bind(row.id).bind(&row.source_id).bind(&row.name).bind(&row.kind).bind(&row.base_url)
            .bind(&row.remote_root).bind(&row.source_subpath).bind(&row.api_key_encrypted)
            .bind(row.revision).bind(row.writable_media).execute(&mut *tx).await.map_err(|_| auth::unavailable())?;
        audit(
            &mut tx,
            &source,
            &input.id.to_string(),
            identity.user.id,
            "arr_instance_reserved",
            None,
            Some(json!({"kind": input.kind,"writable_media": input.writable_media}).to_string()),
        )
        .await?;
        key
    };
    tx.commit().await.map_err(|_| auth::unavailable())?;
    // Persist the connection before enqueueing. A lost reply can be retried with
    // the same ID and key. Hold the source lock while accepting the operation.
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM arr_connections WHERE id=$1 AND source_id=$2 AND managed)",
    )
    .bind(input.id)
    .bind(&source)
    .fetch_one(&mut *tx)
    .await
    .map_err(|_| auth::unavailable())?;
    if !exists {
        return Err(not_found());
    }
    let result: Instance = client
        .request(
            "/v1/instances",
            Some(json!({
                "id": input.id, "source_id": source, "kind": input.kind, "media_path": media,
                "api_key": key, "username": input.username, "password": input.password,
                "writable_media": input.writable_media,
            })),
        )
        .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(json!(result)))
}

#[derive(Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Action {
    Start,
    Stop,
    Restart,
    Remove,
}

pub async fn action(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, id, action)): Path<(String, Uuid, Action)>,
) -> Result<Json<Value>, AppError> {
    owned(&state, &source, identity.user.id).await?;
    let client = authorized(&state, &identity)?;
    auth::rate_limit(
        &state.db,
        format!("arr:manage:{}", identity.user.id),
        30,
        300,
    )
    .await?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    owner_lock(&mut tx, &source, identity.user.id).await?;
    let managed: Option<bool> =
        sqlx::query_scalar("SELECT managed FROM arr_connections WHERE source_id=$1 AND id=$2")
            .bind(&source)
            .bind(id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    if managed != Some(true) {
        return Err(not_found());
    }
    let action = match action {
        Action::Start => "start",
        Action::Stop => "stop",
        Action::Restart => "restart",
        Action::Remove => "remove",
    };
    let result: Instance = client
        .request(
            &format!("/v1/instances/{id}/{action}"),
            Some(json!({"source_id":source})),
        )
        .await?;
    audit(
        &mut tx,
        &source,
        &id.to_string(),
        identity.user.id,
        "arr_instance_action",
        None,
        Some(json!({"action":action}).to_string()),
    )
    .await?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(json!(result)))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::auth_integration::{body, register_user, request, state};
    use axum::{http::Method, routing::post, Router};
    use std::sync::Mutex;

    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "Requires the disposable PostgreSQL integration database"]
    async fn provisioning_requires_admin_ownership_and_allocates_independent_connections(
        pool: sqlx::PgPool,
    ) {
        let mut state = state(pool.clone());
        let root = std::env::temp_dir().join(format!("grimoire-managed-arr-{}", Uuid::new_v4()));
        tokio::fs::create_dir(&root).await.unwrap();
        let socket = root.join("controller.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let calls = Arc::new(Mutex::new(Vec::new()));
        let captured = calls.clone();
        let mock = Router::new().route("/v1/instances", post(move |Json(value): Json<Value>| {
            let captured = captured.clone();
            async move {
                captured.lock().unwrap().push(value.clone());
                Json(json!({"id": value["id"], "status": "provisioning", "private_payload": value}))
            }
        }));
        let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
        let mut config = Config::testing(vec![]);
        config.managed = Client::at(socket);
        Arc::get_mut(&mut state.metadata).unwrap().arr = config;
        let (owner, csrf, user) = register_user(&state, "managed_owner").await;
        let (other, other_csrf, other_user) = register_user(&state, "managed_other").await;
        let user_id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
        let other_id = Uuid::parse_str(other_user["id"].as_str().unwrap()).unwrap();
        let source = Uuid::new_v4().to_string();
        sqlx::query(
            "INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,'Managed fixture',$3)",
        )
        .bind(&source)
        .bind(user_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("UPDATE users SET is_admin=true WHERE id=$1")
            .bind(other_id)
            .execute(&pool)
            .await
            .unwrap();
        let endpoint = format!("/api/sources/{source}/arr/managed");
        let first = Uuid::new_v4();
        let mut payload = json!({"id":first,"name":"Anime","kind":"sonarr","username":"fixture_admin","password":"fixture-management-password"});
        let origin = Some("http://localhost:5173");
        assert_eq!(
            request(
                &state,
                Method::POST,
                &endpoint,
                Some(&owner),
                Some(&csrf),
                origin,
                Some(payload.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &state,
                Method::POST,
                &endpoint,
                Some(&other),
                Some(&other_csrf),
                origin,
                Some(payload.clone())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert!(calls.lock().unwrap().is_empty());
        sqlx::query("UPDATE users SET is_admin=true WHERE id=$1")
            .bind(user_id)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            request(
                &state,
                Method::POST,
                &endpoint,
                Some(&owner),
                None,
                origin,
                Some(payload.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let result = request(
            &state,
            Method::POST,
            &endpoint,
            Some(&owner),
            Some(&csrf),
            origin,
            Some(payload.clone()),
        )
        .await;
        assert_eq!(result.status(), StatusCode::OK);
        assert!(!body(result)
            .await
            .to_string()
            .contains("fixture-management-password"));
        let second = Uuid::new_v4();
        payload["id"] = json!(second);
        payload["writable_media"] = json!(true);
        assert_eq!(
            request(
                &state,
                Method::POST,
                &endpoint,
                Some(&owner),
                Some(&csrf),
                origin,
                Some(payload)
            )
            .await
            .status(),
            StatusCode::OK
        );
        let one = load_connection(&state, &source, first).await.unwrap();
        let two = load_connection(&state, &source, second).await.unwrap();
        assert!(one.managed && two.managed);
        assert!(!one.writable_media && two.writable_media);
        assert_ne!(one.base_url, two.base_url);
        assert_ne!(
            one.secret(&state.metadata.arr).unwrap(),
            two.secret(&state.metadata.arr).unwrap()
        );
        let calls = calls.lock().unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(calls[0]["writable_media"], false);
        assert_eq!(calls[1]["writable_media"], true);
        drop(calls);
        server.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
