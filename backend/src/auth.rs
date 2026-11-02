use crate::{AppError, AppState};
use argon2::{
    password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2,
};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::{HeaderMap, Method, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use axum_extra::extract::{
    cookie::{Cookie, SameSite},
    CookieJar,
};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::{FromRow, PgPool};
use std::{net::SocketAddr, sync::Arc};
use subtle::ConstantTimeEq;
use tokio::sync::Semaphore;
use uuid::Uuid;

#[derive(Clone)]
pub struct Config {
    pub origin: String,
    pub insecure_local: bool,
    pub dummy_hash: String,
}
impl Config {
    pub fn cookie_name(&self) -> &'static str {
        if self.insecure_local {
            "grimoire_session"
        } else {
            "__Host-grimoire_session"
        }
    }
    pub fn from_env(dummy_hash: String) -> Result<Self, String> {
        let origin = std::env::var("GRIMOIRE_APP_ORIGIN")
            .map_err(|_| "Set GRIMOIRE_APP_ORIGIN to the frontend origin.")?;
        let insecure_local = std::env::var("GRIMOIRE_DEV_INSECURE_HTTP").as_deref() == Ok("true");
        Self::parse(origin, insecure_local, dummy_hash)
    }
    fn parse(origin: String, insecure_local: bool, dummy_hash: String) -> Result<Self, String> {
        // An origin has no path, query, credentials, or fragment.
        let uri: axum::http::Uri = origin.parse().map_err(|_| "Invalid application origin.")?;
        let scheme = uri.scheme_str().ok_or("Origin must include a scheme.")?;
        let authority = uri.authority().ok_or("Origin must include a host.")?;
        if authority.as_str().contains('@')
            || uri.path_and_query().is_some_and(|p| p.as_str() != "/")
            || origin.ends_with('/')
        {
            return Err(
                "Use an origin without credentials, path, query, fragment, or trailing slash."
                    .into(),
            );
        }
        if insecure_local {
            if scheme != "http" || !matches!(authority.host(), "localhost" | "127.0.0.1" | "[::1]")
            {
                return Err(
                    "Insecure HTTP is allowed only for explicit loopback development origins."
                        .into(),
                );
            }
        } else if scheme != "https" {
            return Err(
                "HTTPS is required unless loopback development mode is explicitly enabled.".into(),
            );
        }
        Ok(Self {
            origin,
            insecure_local,
            dummy_hash,
        })
    }
}

#[derive(Clone, Debug, Serialize, FromRow)]
pub struct User {
    pub id: Uuid,
    pub username: String,
    pub is_admin: bool,
}
#[derive(Clone)]
pub struct Identity {
    pub user: User,
    pub session_hash: String,
    pub csrf_token: String,
}
#[derive(Serialize)]
pub struct AuthResponse {
    pub user: User,
    pub csrf_token: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub username: String,
    pub password: String,
}
#[derive(FromRow)]
struct LoginRecord {
    id: Uuid,
    username: String,
    is_admin: bool,
    password_hash: String,
    disabled: bool,
}

pub fn unavailable() -> AppError {
    AppError(
        StatusCode::SERVICE_UNAVAILABLE,
        "The account service is unavailable. Try again later.".into(),
    )
}
pub fn unauthorized() -> AppError {
    AppError(StatusCode::UNAUTHORIZED, "Please sign in.".into())
}
pub fn digest(value: &str) -> String {
    hex::encode(Sha256::digest(value.as_bytes()))
}
fn token() -> String {
    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
pub fn username(input: &str) -> Result<String, AppError> {
    let name = input.trim().to_ascii_lowercase();
    if !(3..=32).contains(&name.len())
        || !name
            .bytes()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_' || c == b'-')
    {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Use a username of 3–32 letters, digits, underscores, or hyphens.".into(),
        ));
    }
    Ok(name)
}
pub fn validate_password(password: &str) -> Result<(), AppError> {
    if !(15..=128).contains(&password.chars().count()) || password.len() > 1024 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Use a password of 15–128 characters.".into(),
        ));
    }
    Ok(())
}
pub fn hash_password(password: &str) -> Result<String, AppError> {
    Argon2::default()
        .hash_password(password.as_bytes(), &SaltString::generate(&mut OsRng))
        .map(|hash| hash.to_string())
        .map_err(|_| unavailable())
}
async fn hash_bounded(semaphore: Arc<Semaphore>, password: String) -> Result<String, AppError> {
    let permit = semaphore.try_acquire_owned().map_err(|_| {
        AppError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many account requests. Try again shortly.".into(),
        )
    })?;
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        hash_password(&password)
    })
    .await
    .map_err(|_| unavailable())?
}

pub fn require_origin(headers: &HeaderMap, expected: &str) -> Result<(), AppError> {
    if headers.get("origin").and_then(|v| v.to_str().ok()) != Some(expected) {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Request origin is not allowed.".into(),
        ));
    }
    Ok(())
}
fn csrf_matches(headers: &HeaderMap, expected: &str) -> bool {
    let supplied = headers
        .get("x-csrf-token")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");
    supplied.as_bytes().ct_eq(expected.as_bytes()).into()
}
pub(crate) async fn rate_limit(
    db: &PgPool,
    key: String,
    limit: i32,
    seconds: i32,
) -> Result<(), AppError> {
    sqlx::query("DELETE FROM auth_rate_limits WHERE expires_at < now()")
        .execute(db)
        .await
        .map_err(|_| unavailable())?;
    let count: i32 = sqlx::query_scalar("INSERT INTO auth_rate_limits (key, attempts, expires_at) VALUES ($1, 1, now() + $2::int * interval '1 second') ON CONFLICT (key) DO UPDATE SET attempts = LEAST(auth_rate_limits.attempts + 1, 1000000) RETURNING attempts")
        .bind(key).bind(seconds).fetch_one(db).await.map_err(|_| unavailable())?;
    if count > limit {
        return Err(AppError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many attempts. Please try again later.".into(),
        ));
    }
    Ok(())
}
async fn audit(db: &PgPool, user: Option<Uuid>, event: &str) -> Result<(), AppError> {
    sqlx::query("INSERT INTO auth_events (user_id, event) VALUES ($1, $2)")
        .bind(user)
        .bind(event)
        .execute(db)
        .await
        .map_err(|_| unavailable())?;
    Ok(())
}

// All state-changing API requests require the configured frontend origin.
// Login/registration are bounded before password hashing; forwarding headers are not trusted.
pub async fn origin_and_limits(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Result<Response, AppError> {
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) {
        require_origin(request.headers(), &state.config.origin)?;
    }
    if matches!(
        request.uri().path(),
        "/api/auth/login" | "/api/auth/register"
    ) && request.method() == Method::POST
    {
        let address = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .ok_or_else(unavailable)?
            .0
            .ip();
        let registration = request.uri().path().ends_with("register");
        let (kind, count, seconds) = if registration {
            ("register", 5, 3600)
        } else {
            ("login", 30, 900)
        };
        rate_limit(
            &state.db,
            format!("{kind}:ip:{}", digest(&address.to_string())),
            count,
            seconds,
        )
        .await?;
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert("cache-control", "private, no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    Ok(response)
}

pub async fn session_identity(state: &AppState, jar: &CookieJar) -> Result<Identity, AppError> {
    let raw = jar
        .get(state.config.cookie_name())
        .ok_or_else(unauthorized)?
        .value();
    if raw.len() != 64 || !raw.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(unauthorized());
    }
    let hash = digest(raw);
    // Atomic idle-expiry check and touch; disabled users and expired sessions fail closed.
    let row: Option<(Uuid, String)> = sqlx::query_as("UPDATE sessions SET last_seen = now() WHERE token_hash = $1 AND expires_at > now() AND last_seen > now() - interval '30 minutes' AND EXISTS (SELECT 1 FROM users WHERE users.id = sessions.user_id AND NOT disabled) RETURNING user_id, csrf_token")
        .bind(&hash).fetch_optional(&state.db).await.map_err(|_| unavailable())?;
    let (user_id, csrf_token) = row.ok_or_else(unauthorized)?;
    let user = sqlx::query_as::<_, User>(
        "SELECT id, username, is_admin FROM users WHERE id=$1 AND NOT disabled",
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| unavailable())?
    .ok_or_else(unauthorized)?;
    Ok(Identity {
        user,
        session_hash: hash,
        csrf_token,
    })
}

pub async fn require_session(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let identity = session_identity(&state, &CookieJar::from_headers(request.headers())).await?;
    if !matches!(
        *request.method(),
        Method::GET | Method::HEAD | Method::OPTIONS
    ) && !csrf_matches(request.headers(), &identity.csrf_token)
    {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Refresh your session and try again.".into(),
        ));
    }
    request.extensions_mut().insert(identity);
    Ok(next.run(request).await)
}
fn session_cookie(config: &Config, value: String) -> Cookie<'static> {
    Cookie::build((config.cookie_name(), value))
        .path("/")
        .http_only(true)
        .secure(!config.insecure_local)
        .same_site(SameSite::Strict)
        .max_age(time::Duration::hours(12))
        .build()
}
async fn new_session(
    state: &AppState,
    jar: CookieJar,
    user: User,
) -> Result<(CookieJar, Json<AuthResponse>), AppError> {
    let raw = token();
    let csrf = token();
    let mut tx = state.db.begin().await.map_err(|_| unavailable())?;
    if let Some(old) = jar.get(state.config.cookie_name()) {
        sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
            .bind(digest(old.value()))
            .execute(&mut *tx)
            .await
            .map_err(|_| unavailable())?;
    }
    sqlx::query("DELETE FROM sessions WHERE expires_at < now() OR last_seen < now() - interval '30 minutes'").execute(&mut *tx).await.map_err(|_| unavailable())?;
    sqlx::query("INSERT INTO sessions (token_hash, user_id, csrf_token) VALUES ($1,$2,$3)")
        .bind(digest(&raw))
        .bind(user.id)
        .bind(&csrf)
        .execute(&mut *tx)
        .await
        .map_err(|_| unavailable())?;
    // Bound simultaneous sessions per account, keeping the most recent ten.
    sqlx::query("DELETE FROM sessions WHERE token_hash IN (SELECT token_hash FROM sessions WHERE user_id=$1 ORDER BY created_at DESC OFFSET 10)").bind(user.id).execute(&mut *tx).await.map_err(|_| unavailable())?;
    tx.commit().await.map_err(|_| unavailable())?;
    Ok((
        jar.add(session_cookie(&state.config, raw)),
        Json(AuthResponse {
            user,
            csrf_token: csrf,
        }),
    ))
}

pub async fn register(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(credentials): Json<Credentials>,
) -> Result<impl IntoResponse, AppError> {
    let name = username(&credentials.username)?;
    validate_password(&credentials.password)?;
    let hash = hash_bounded(state.hashing.clone(), credentials.password).await?;
    let user = sqlx::query_as::<_, User>("INSERT INTO users (id,username,password_hash) VALUES ($1,$2,$3) ON CONFLICT (username) DO NOTHING RETURNING id, username, is_admin")
        .bind(Uuid::new_v4()).bind(name).bind(hash).fetch_optional(&state.db).await.map_err(|_| unavailable())?
        .ok_or_else(|| AppError(StatusCode::CONFLICT, "That username is unavailable.".into()))?;
    audit(&state.db, Some(user.id), "registered").await?;
    let (jar, body) = new_session(&state, jar, user).await?;
    Ok((StatusCode::CREATED, jar, body))
}
pub async fn login(
    State(state): State<AppState>,
    jar: CookieJar,
    Json(credentials): Json<Credentials>,
) -> Result<impl IntoResponse, AppError> {
    let invalid = || {
        AppError(
            StatusCode::UNAUTHORIZED,
            "Invalid username or password.".into(),
        )
    };
    let name = username(&credentials.username).map_err(|_| invalid())?;
    if credentials.password.len() > 1024 {
        return Err(invalid());
    }
    rate_limit(
        &state.db,
        format!("login:account:{}", digest(&name)),
        10,
        900,
    )
    .await?;
    let record = sqlx::query_as::<_, LoginRecord>(
        "SELECT id,username,is_admin,password_hash,disabled FROM users WHERE username=$1",
    )
    .bind(name)
    .fetch_optional(&state.db)
    .await
    .map_err(|_| unavailable())?;
    let hash = record
        .as_ref()
        .map(|r| r.password_hash.clone())
        .unwrap_or_else(|| state.config.dummy_hash.clone());
    let permit = state.hashing.clone().try_acquire_owned().map_err(|_| {
        AppError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many account requests. Try again shortly.".into(),
        )
    })?;
    let valid = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        PasswordHash::new(&hash).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(credentials.password.as_bytes(), &hash)
                .is_ok()
        })
    })
    .await
    .map_err(|_| unavailable())?;
    if !valid || record.as_ref().is_none_or(|r| r.disabled) {
        audit(&state.db, None, "login_failed").await?;
        return Err(invalid());
    }
    let record = record.ok_or_else(invalid)?;
    audit(&state.db, Some(record.id), "login_succeeded").await?;
    new_session(
        &state,
        jar,
        User {
            id: record.id,
            username: record.username,
            is_admin: record.is_admin,
        },
    )
    .await
}
pub async fn me(axum::Extension(identity): axum::Extension<Identity>) -> Json<AuthResponse> {
    Json(AuthResponse {
        user: identity.user,
        csrf_token: identity.csrf_token,
    })
}
pub async fn logout(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<Identity>,
    jar: CookieJar,
) -> Result<impl IntoResponse, AppError> {
    sqlx::query("DELETE FROM sessions WHERE token_hash=$1")
        .bind(identity.session_hash)
        .execute(&state.db)
        .await
        .map_err(|_| unavailable())?;
    audit(&state.db, Some(identity.user.id), "logged_out").await?;
    let mut cookie = session_cookie(&state.config, String::new());
    cookie.make_removal();
    Ok((jar.add(cookie), StatusCode::NO_CONTENT))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_policy_hashes_and_cookies() {
        assert!(validate_password("short").is_err());
        let hash = hash_password("a long test passphrase").unwrap_or_else(|_| panic!("hash"));
        assert!(hash.starts_with("$argon2id$"));
        assert!(Argon2::default()
            .verify_password(b"wrong", &PasswordHash::new(&hash).unwrap())
            .is_err());
        let config = Config {
            origin: "https://grimoire.test".into(),
            insecure_local: false,
            dummy_hash: hash,
        };
        let cookie = session_cookie(&config, token());
        assert_eq!(cookie.http_only(), Some(true));
        assert_eq!(cookie.secure(), Some(true));
        assert_eq!(cookie.same_site(), Some(SameSite::Strict));
        assert_eq!(cookie.path(), Some("/"));
        assert!(cookie.name().starts_with("__Host-"));
    }
    #[test]
    fn configuration_requires_https_or_explicit_local_development() {
        assert!(Config::parse("https://grimoire.test".into(), false, String::new()).is_ok());
        assert!(Config::parse("http://localhost:5173".into(), true, String::new()).is_ok());
        assert!(Config::parse("http://localhost:5173".into(), false, String::new()).is_err());
        for origin in [
            "http://attacker.test",
            "http://localhost:5173/",
            "http://localhost:5173/path",
            "http://localhost:5173?query=1",
        ] {
            assert!(Config::parse(origin.into(), true, String::new()).is_err());
        }
    }

    #[test]
    fn origins_csrf_and_usernames() {
        let mut headers = HeaderMap::new();
        assert!(require_origin(&headers, "https://grimoire.test").is_err());
        headers.insert("origin", "https://attacker.test".parse().unwrap());
        assert!(require_origin(&headers, "https://grimoire.test").is_err());
        headers.insert("origin", "https://grimoire.test".parse().unwrap());
        assert!(require_origin(&headers, "https://grimoire.test").is_ok());
        assert!(!csrf_matches(&headers, "secret"));
        headers.insert("x-csrf-token", "secret".parse().unwrap());
        assert!(csrf_matches(&headers, "secret"));
        assert_eq!(username(" ALICE ").unwrap_or_default(), "alice");
        assert!(username("../alice").is_err());
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PasswordChange {
    current_password: String,
    new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    axum::Extension(identity): axum::Extension<Identity>,
    jar: CookieJar,
    Json(change): Json<PasswordChange>,
) -> Result<impl IntoResponse, AppError> {
    validate_password(&change.new_password)?;
    if change.current_password.len() > 1024 {
        return Err(AppError(
            StatusCode::BAD_REQUEST,
            "Current password is incorrect.".into(),
        ));
    }
    rate_limit(
        &state.db,
        format!("password:user:{}", identity.user.id),
        10,
        900,
    )
    .await?;
    let old: String =
        sqlx::query_scalar("SELECT password_hash FROM users WHERE id=$1 AND NOT disabled")
            .bind(identity.user.id)
            .fetch_optional(&state.db)
            .await
            .map_err(|_| unavailable())?
            .ok_or_else(unauthorized)?;
    let permit = state.hashing.clone().try_acquire_owned().map_err(|_| {
        AppError(
            StatusCode::TOO_MANY_REQUESTS,
            "Too many account requests. Try again shortly.".into(),
        )
    })?;
    let old_for_verify = old.clone();
    let new_hash = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let valid = PasswordHash::new(&old_for_verify).ok().is_some_and(|hash| {
            Argon2::default()
                .verify_password(change.current_password.as_bytes(), &hash)
                .is_ok()
        });
        if !valid {
            return Err(AppError(
                StatusCode::BAD_REQUEST,
                "Current password is incorrect.".into(),
            ));
        }
        hash_password(&change.new_password)
    })
    .await
    .map_err(|_| unavailable())??;
    let mut tx = state.db.begin().await.map_err(|_| unavailable())?;
    let updated = sqlx::query(
        "UPDATE users SET password_hash=$1 WHERE id=$2 AND password_hash=$3 AND NOT disabled",
    )
    .bind(new_hash)
    .bind(identity.user.id)
    .bind(old)
    .execute(&mut *tx)
    .await
    .map_err(|_| unavailable())?;
    if updated.rows_affected() != 1 {
        return Err(unauthorized());
    }
    sqlx::query("DELETE FROM sessions WHERE user_id=$1")
        .bind(identity.user.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| unavailable())?;
    sqlx::query("INSERT INTO auth_events(user_id,event) VALUES ($1,'password_changed')")
        .bind(identity.user.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| unavailable())?;
    tx.commit().await.map_err(|_| unavailable())?;
    new_session(&state, jar, identity.user).await
}
