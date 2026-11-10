use super::*;
use axum::{
    body::{to_bytes, Body},
    extract::ConnectInfo,
    http::{Method, Request},
};
use serde_json::{json, Value};
use sqlx::PgPool;
use tower::ServiceExt;
use uuid::Uuid;

const ORIGIN: &str = "http://localhost:5173";
pub(super) fn state(pool: PgPool) -> AppState {
    AppState {
        network: Arc::new(crate::security::Network::default()),
        tls: Arc::new(crate::security::tls::Runtime::default()),
        db: pool,
        config: Arc::new(auth::Config {
            additional_origins: Vec::new(),
            origin: ORIGIN.into(),
            insecure_local: true,
            dummy_hash: auth::hash_password("a dummy password used for tests")
                .unwrap_or_else(|_| panic!("hash")),
        }),
        hashing: Arc::new(tokio::sync::Semaphore::new(4)),
        catalog_scans: Arc::new(tokio::sync::Semaphore::new(2)),
        metadata: Arc::new(metadata::Runtime::new()),
        video_audio: Arc::new(crate::video_audio::Runtime::new()),
        transcode: Arc::new(crate::transcode::Runtime::new()),
    }
}
pub(super) async fn request(
    state: &AppState,
    method: Method,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    origin: Option<&str>,
    body: Option<Value>,
) -> Response {
    let mut builder = Request::builder()
        .method(method)
        .uri(path)
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ));
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    if let Some(origin) = origin {
        builder = builder.header("origin", origin);
    }
    if body.is_some() {
        builder = builder.header("content-type", "application/json");
    }
    app(state.clone())
        .oneshot(
            builder
                .body(Body::from(body.map(|b| b.to_string()).unwrap_or_default()))
                .unwrap(),
        )
        .await
        .unwrap()
}
pub(super) async fn body(response: Response) -> Value {
    serde_json::from_slice(&to_bytes(response.into_body(), 1024 * 1024).await.unwrap()).unwrap()
}
async fn session(response: Response, status: StatusCode) -> (String, String, Value) {
    assert_eq!(response.status(), status);
    let cookie = response.headers()["set-cookie"]
        .to_str()
        .unwrap()
        .split(';')
        .next()
        .unwrap()
        .to_string();
    let data = body(response).await;
    (
        cookie,
        data["csrf_token"].as_str().unwrap().to_string(),
        data["user"].clone(),
    )
}
async fn seed_admin(pool: &PgPool) {
    sqlx::query(
        "INSERT INTO users(id,username,password_hash,is_admin) VALUES ($1,'admin',$2,true)",
    )
    .bind(Uuid::new_v4())
    .bind(auth::hash_password("administrator test passphrase").unwrap_or_default())
    .execute(pool)
    .await
    .unwrap();
}
async fn login(state: &AppState, name: &str, password: &str) -> (String, String, Value) {
    session(
        request(
            state,
            Method::POST,
            "/api/auth/login",
            None,
            None,
            Some(ORIGIN),
            Some(json!({"username":name,"password":password})),
        )
        .await,
        StatusCode::OK,
    )
    .await
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn registration_and_source_access_are_default_deny(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    for path in [
        "/api/auth/me",
        "/api/sources",
        "/api/sources/known/media",
        "/api/sources/known/audio?path=track.mp3",
        "/api/sources/known/video?path=clip.mp4",
        "/api/sources/known/book?path=story.epub",
    ] {
        assert_eq!(
            request(&state, Method::GET, path, None, None, None, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/sources",
            Some("grimoire_session=forged"),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/auth/register",
            None,
            None,
            Some("https://attacker.test"),
            Some(json!({"username":"alice","password":"a long user test passphrase"}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let rejected = request(
        &state,
        Method::POST,
        "/api/auth/register",
        None,
        None,
        Some(ORIGIN),
        Some(json!({"username":"alice","password":"a long user test passphrase","is_admin":true})),
    )
    .await;
    assert_eq!(rejected.status(), StatusCode::UNPROCESSABLE_ENTITY);
    let (alice, csrf, user) = session(
        request(
            &state,
            Method::POST,
            "/api/auth/register",
            None,
            None,
            Some(ORIGIN),
            Some(json!({"username":"ALICE","password":"a long user test passphrase"})),
        )
        .await,
        StatusCode::CREATED,
    )
    .await;
    assert_eq!(user["is_admin"], false);
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                "/api/sources",
                Some(&alice),
                None,
                None,
                None
            )
            .await
        )
        .await,
        json!([])
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/sources",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"path":"/tmp"}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let (admin, admin_csrf, _) = login(&state, "admin", "administrator test passphrase").await;
    let root = std::env::temp_dir().join(format!("grimoire-ownership-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    for name in ["track.mp3", "clip.mp4", "story.epub", "comic.cbz"] {
        fs::write(root.join(name), b"test content").unwrap();
    }
    let payload = json!({"path":root.to_str().unwrap()});
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/sources",
            Some(&admin),
            None,
            Some(ORIGIN),
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
            "/api/sources",
            Some(&admin),
            Some(&admin_csrf),
            None,
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let response = request(
        &state,
        Method::POST,
        "/api/sources",
        Some(&admin),
        Some(&admin_csrf),
        Some(ORIGIN),
        Some(payload),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let source = body(response).await;
    let id = source["id"].as_str().unwrap();
    for suffix in [
        "media",
        "audio?path=track.mp3",
        "video?path=clip.mp4",
        "book?path=story.epub",
        "book?path=comic.cbz",
    ] {
        let path = format!("/api/sources/{id}/{suffix}");
        assert_eq!(
            request(&state, Method::GET, &path, Some(&admin), None, None, None)
                .await
                .status(),
            StatusCode::OK
        );
        assert_eq!(
            request(&state, Method::GET, &path, Some(&alice), None, None, None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("/api/sources/{id}"),
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("/api/sources/{id}"),
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(root.join("track.mp3").exists());
    assert_eq!(
        request(
            &state,
            Method::GET,
            &format!("/api/sources/{id}/audio?path=track.mp3"),
            Some(&admin),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn sessions_rotate_expire_revoke_and_password_changes(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    let (first, _, _) = login(&state, "admin", "administrator test passphrase").await;
    let first_raw = first.split_once('=').unwrap().1;
    let stored: String = sqlx::query_scalar("SELECT token_hash FROM sessions LIMIT 1")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(stored, first_raw);
    assert_eq!(stored, auth::digest(first_raw));
    let response = request(
        &state,
        Method::POST,
        "/api/auth/login",
        Some(&first),
        None,
        Some(ORIGIN),
        Some(json!({"username":"admin","password":"administrator test passphrase"})),
    )
    .await;
    let (second, csrf, _) = session(response, StatusCode::OK).await;
    assert_ne!(first, second);
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/auth/me",
            Some(&first),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/auth/logout",
            Some(&second),
            Some("wrong"),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let (other, _, _) = login(&state, "admin", "administrator test passphrase").await;
    let response = request(&state, Method::POST, "/api/auth/password", Some(&second), Some(&csrf), Some(ORIGIN), Some(json!({"current_password":"administrator test passphrase","new_password":"a changed administrator passphrase"}))).await;
    let (third, csrf, _) = session(response, StatusCode::OK).await;
    for cookie in [&second, &other] {
        assert_eq!(
            request(
                &state,
                Method::GET,
                "/api/auth/me",
                Some(cookie),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/auth/logout",
            Some(&third),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/auth/me",
            Some(&third),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let (idle, _, _) = login(&state, "admin", "a changed administrator passphrase").await;
    sqlx::query("UPDATE sessions SET last_seen=now()-interval '31 minutes'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/auth/me",
            Some(&idle),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let (expired, _, _) = login(&state, "admin", "a changed administrator passphrase").await;
    sqlx::query("UPDATE sessions SET expires_at=now()-interval '1 second'")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/auth/me",
            Some(&expired),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let (disabled, _, _) = login(&state, "admin", "a changed administrator passphrase").await;
    sqlx::query("UPDATE users SET disabled=true")
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/sources",
            Some(&disabled),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn authentication_errors_are_generic_and_rate_limited(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    let mut messages = Vec::new();
    for name in ["admin", "unknown"] {
        let response = request(
            &state,
            Method::POST,
            "/api/auth/login",
            None,
            None,
            Some(ORIGIN),
            Some(json!({"username":name,"password":"incorrect test passphrase"})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        messages.push(body(response).await);
    }
    assert_eq!(messages[0], messages[1]);
    sqlx::query("INSERT INTO auth_rate_limits(key,attempts,expires_at) VALUES ($1,10,now()+interval '15 minutes') ON CONFLICT (key) DO UPDATE SET attempts=10").bind(format!("login:account:{}",auth::digest("admin"))).execute(&pool).await.unwrap();
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/auth/login",
            None,
            None,
            Some(ORIGIN),
            Some(json!({"username":"admin","password":"administrator test passphrase"}))
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
    sqlx::query("INSERT INTO auth_rate_limits(key,attempts,expires_at) VALUES ($1,5,now()+interval '1 hour')").bind(format!("register:ip:{}",auth::digest("127.0.0.1"))).execute(&pool).await.unwrap();
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/auth/register",
            None,
            None,
            Some(ORIGIN),
            Some(json!({"username":"bob","password":"a new registration passphrase"}))
        )
        .await
        .status(),
        StatusCode::TOO_MANY_REQUESTS
    );
}

pub(super) async fn register_user(state: &AppState, name: &str) -> (String, String, Value) {
    session(
        request(
            state,
            Method::POST,
            "/api/auth/register",
            None,
            None,
            Some(ORIGIN),
            Some(json!({"username":name,"password":"a long shared source passphrase"})),
        )
        .await,
        StatusCode::CREATED,
    )
    .await
}
fn shared_media_fixture() -> std::path::PathBuf {
    let root = std::env::temp_dir().join(format!("grimoire-permissions-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    for name in ["track.mp3", "clip.mp4", "story.epub", "comic.cbz"] {
        fs::write(root.join(name), b"shared test content").unwrap();
    }
    root
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn source_permissions_separate_visibility_media_and_acquisition_and_revoke(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    let (admin, admin_csrf, admin_user) =
        login(&state, "admin", "administrator test passphrase").await;
    let (alice, alice_csrf, _) = register_user(&state, "alice").await;
    let root = shared_media_fixture();
    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Shared source',$3)")
        .bind(&id)
        .bind(Uuid::parse_str(admin_user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let endpoint = format!("/api/sources/{id}/permissions/alice");
    let roster = format!("/api/sources/{id}/permissions");
    assert_eq!(
        request(&state, Method::GET, &roster, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let suffixes = [
        "media",
        "audio?path=track.mp3",
        "video?path=clip.mp4",
        "book?path=story.epub",
        "book?path=comic.cbz",
    ];
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                "/api/sources",
                Some(&alice),
                None,
                None,
                None
            )
            .await
        )
        .await,
        json!([])
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&admin),
            None,
            Some(ORIGIN),
            Some(json!({}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let defaults =
        body(request(&state, Method::GET, &roster, Some(&admin), None, None, None).await).await;
    assert_eq!(defaults[0]["can_discover"], false);
    assert_eq!(defaults[0]["can_access_media"], false);
    assert_eq!(defaults[0]["acquisition_policy"], "auto_deny");
    for policy in ["manual_approval", "auto_accept"] {
        let response = request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":true,"can_access_media":false,"acquisition_policy":policy})),
        )
        .await;
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
        let sources = body(
            request(
                &state,
                Method::GET,
                "/api/sources",
                Some(&alice),
                None,
                None,
                None,
            )
            .await,
        )
        .await;
        assert_eq!(sources.as_array().unwrap().len(), 1);
        assert_eq!(sources[0]["can_access_media"], false);
        assert_eq!(sources[0]["acquisition_policy"], policy);
        assert_eq!(sources[0]["is_owner"], false);
        assert!(sources[0].get("path").is_none());
        for suffix in suffixes {
            assert_eq!(
                request(
                    &state,
                    Method::GET,
                    &format!("/api/sources/{id}/{suffix}"),
                    Some(&alice),
                    None,
                    None,
                    None
                )
                .await
                .status(),
                StatusCode::NOT_FOUND
            );
        }
    }
    assert_eq!(
        request(&state, Method::GET, &roster, Some(&alice), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&alice),
            Some(&alice_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":true,"can_access_media":true}))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &endpoint,
            Some(&alice),
            Some(&alice_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":false,"can_access_media":true}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":false,"acquisition_policy":"auto_accept"}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    for policy in ["auto_deny", "manual_approval"] {
        assert_eq!(
            request(
                &state,
                Method::PUT,
                &endpoint,
                Some(&admin),
                Some(&admin_csrf),
                Some(ORIGIN),
                Some(
                    json!({"can_discover":true,"can_access_media":true,"acquisition_policy":policy})
                )
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        for suffix in suffixes {
            assert_eq!(
                request(
                    &state,
                    Method::GET,
                    &format!("/api/sources/{id}/{suffix}"),
                    Some(&alice),
                    None,
                    None,
                    None
                )
                .await
                .status(),
                StatusCode::OK
            );
        }
    }
    let range_request = Request::builder()
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .uri(format!("/api/sources/{id}/audio?path=track.mp3"))
        .header("cookie", &alice)
        .header("range", "bytes=0-5")
        .body(Body::empty())
        .unwrap();
    let range_response = app(state.clone()).oneshot(range_request).await.unwrap();
    assert_eq!(range_response.status(), StatusCode::PARTIAL_CONTENT);
    assert_eq!(
        to_bytes(range_response.into_body(), 32)
            .await
            .unwrap()
            .as_ref(),
        b"shared"
    );
    // Revoke media access while retaining visibility; every media endpoint denies again.
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":true}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    for suffix in suffixes {
        assert_eq!(
            request(
                &state,
                Method::GET,
                &format!("/api/sources/{id}/{suffix}"),
                Some(&alice),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
    }
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                "/api/sources",
                Some(&alice),
                None,
                None,
                None
            )
            .await
        )
        .await,
        json!([])
    );
    let audit: (String, Option<String>) = sqlx::query_as("SELECT action,before_state FROM source_permission_audit WHERE source_id=$1 ORDER BY id DESC LIMIT 1").bind(&id).fetch_one(&pool).await.unwrap();
    assert_eq!(audit.0, "revoked");
    assert!(audit.1.unwrap().contains("can_discover"));
    // Owner visibility/media management is intrinsic; their acquisition policy still defaults to deny.
    let owner_endpoint = format!("/api/sources/{id}/permissions/admin");
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &owner_endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(request(&state, Method::PUT, &owner_endpoint, Some(&admin), Some(&admin_csrf), Some(ORIGIN), Some(json!({"can_discover":true,"can_access_media":true,"acquisition_policy":"auto_accept"}))).await.status(), StatusCode::NO_CONTENT);
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &owner_endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let owner_sources = body(
        request(
            &state,
            Method::GET,
            "/api/sources",
            Some(&admin),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(owner_sources[0]["can_access_media"], true);
    assert_eq!(owner_sources[0]["acquisition_policy"], "auto_deny");
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn only_source_owner_manages_grants_and_disconnect_revokes_them(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    let (admin, admin_csrf, _) = login(&state, "admin", "administrator test passphrase").await;
    let (alice, alice_csrf, alice_user) = register_user(&state, "alice").await;
    let (bob, bob_csrf, _) = register_user(&state, "bob").await;
    let root = shared_media_fixture();
    let id = Uuid::new_v4().to_string();
    // Model a future user-owned source: grant management depends on ownership, not administrator status.
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'User owned source',$3)")
        .bind(&id)
        .bind(Uuid::parse_str(alice_user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let roster = format!("/api/sources/{id}/permissions");
    let endpoint = format!("{roster}/bob");
    let grant = json!({"can_discover":true,"can_access_media":true});
    assert_eq!(
        request(&state, Method::GET, &roster, Some(&admin), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(grant.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &endpoint,
            Some(&alice),
            Some(&alice_csrf),
            Some(ORIGIN),
            Some(grant)
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let stream = format!("/api/sources/{id}/audio?path=track.mp3");
    assert_eq!(
        request(&state, Method::GET, &stream, Some(&admin), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(&state, Method::GET, &stream, Some(&bob), None, None, None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("/api/sources/{id}"),
            Some(&bob),
            Some(&bob_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("/api/sources/{id}"),
            Some(&alice),
            Some(&alice_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(&state, Method::GET, &stream, Some(&bob), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let remaining: i64 =
        sqlx::query_scalar("SELECT count(*) FROM source_permissions WHERE source_id=$1")
            .bind(&id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(remaining, 0);
    let revoked: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM source_permission_audit WHERE source_id=$1 AND action='revoked'",
    )
    .bind(&id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(revoked, 1);
    assert!(root.join("track.mp3").exists());
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn catalog_library_and_activity_respect_grants_and_user_isolation(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    let (admin, admin_csrf, admin_user) =
        login(&state, "admin", "administrator test passphrase").await;
    let (alice, csrf, alice_user) = register_user(&state, "alice").await;
    let (bob, bob_csrf, _) = register_user(&state, "bob").await;
    let root = shared_media_fixture();
    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Catalog source',$3)")
        .bind(&id)
        .bind(Uuid::parse_str(admin_user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let reference = json!({"source_id":id,"relative_path":"track.mp3"});
    for path in ["/api/catalog", "/api/library", "/api/activity"] {
        assert_eq!(
            request(&state, Method::GET, path, None, None, None, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert!(
            body(request(&state, Method::GET, path, Some(&alice), None, None, None).await).await
                ["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/library",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(reference.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let permission = format!("/api/sources/{id}/permissions/alice");
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &permission,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":true,"acquisition_policy":"auto_accept"}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(body(
        request(
            &state,
            Method::GET,
            "/api/catalog",
            Some(&alice),
            None,
            None,
            None
        )
        .await
    )
    .await["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/activity/open",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(reference.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &permission,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":true,"can_access_media":true}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let bob_permission = format!("/api/sources/{id}/permissions/bob");
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &bob_permission,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":true,"can_access_media":true}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    for (method, endpoint) in [
        (Method::PUT, "/api/library"),
        (Method::POST, "/api/activity/open"),
    ] {
        assert_eq!(
            request(
                &state,
                method,
                endpoint,
                Some(&alice),
                Some(&csrf),
                Some(ORIGIN),
                Some(json!({"source_id":id,"relative_path":"x".repeat(5000)}))
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
    }
    let catalog = body(
        request(
            &state,
            Method::GET,
            "/api/catalog",
            Some(&alice),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(catalog["items"].as_array().unwrap().len(), 4);
    assert!(!catalog.to_string().contains(root.to_str().unwrap()));
    assert!(catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["saved_at"].is_null() && item["last_opened_at"].is_null()));
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/library",
            Some(&alice),
            None,
            Some(ORIGIN),
            Some(reference.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let saved = body(
        request(
            &state,
            Method::PUT,
            "/api/library",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(reference.clone()),
        )
        .await,
    )
    .await;
    assert!(saved["saved_at"].is_number());
    assert!(saved["last_opened_at"].is_null());
    let again = body(
        request(
            &state,
            Method::PUT,
            "/api/library",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(reference.clone()),
        )
        .await,
    )
    .await;
    assert_eq!(saved["saved_at"], again["saved_at"]);
    let library = body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&alice),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(library["items"][0]["name"], "track.mp3");
    assert!(body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&bob),
            None,
            None,
            None
        )
        .await
    )
    .await["items"]
        .as_array()
        .unwrap()
        .is_empty());
    let key = saved["file_key"].as_str().unwrap();
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("/api/library/{key}"),
            Some(&bob),
            Some(&bob_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let bob_catalog = body(
        request(
            &state,
            Method::GET,
            "/api/catalog",
            Some(&bob),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(bob_catalog["items"].as_array().unwrap().len(), 4);
    assert!(bob_catalog["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["saved_at"].is_null() && item["last_opened_at"].is_null()));
    let still_saved = body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&alice),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(still_saved["items"].as_array().unwrap().len(), 1);
    let opened = body(
        request(
            &state,
            Method::POST,
            "/api/activity/open",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(reference.clone()),
        )
        .await,
    )
    .await;
    assert!(opened["last_opened_at"].is_number());
    assert_eq!(opened["saved_at"], saved["saved_at"]);
    let activity = body(
        request(
            &state,
            Method::GET,
            "/api/activity",
            Some(&alice),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(activity["items"][0]["name"], "track.mp3");
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/activity/open",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"source_id":id,"relative_path":"../track.mp3"}))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/activity/open",
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"source_id":id,"relative_path":"missing.mp3"}))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &permission,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    for path in ["/api/catalog", "/api/library", "/api/activity"] {
        assert!(
            body(request(&state, Method::GET, path, Some(&alice), None, None, None).await).await
                ["items"]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_media WHERE user_id=$1")
        .bind(Uuid::parse_str(alice_user["id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(count, 1); // Personal records persist, but cannot reveal revoked media.
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &permission,
            Some(&admin),
            Some(&admin_csrf),
            Some(ORIGIN),
            Some(json!({"can_discover":true,"can_access_media":true}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                "/api/library",
                Some(&alice),
                None,
                None,
                None
            )
            .await
        )
        .await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("/api/library/{key}"),
            Some(&alice),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&alice),
            None,
            None,
            None
        )
        .await
    )
    .await["items"]
        .as_array()
        .unwrap()
        .is_empty());
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                "/api/activity",
                Some(&alice),
                None,
                None,
                None
            )
            .await
        )
        .await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn library_handles_missing_files_and_offline_sources_without_changing_files(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    let (admin, csrf, user) = login(&state, "admin", "administrator test passphrase").await;
    let root = shared_media_fixture();
    let id = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Offline test',$3)")
        .bind(&id)
        .bind(Uuid::parse_str(user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let reference = json!({"source_id":id,"relative_path":"story.epub"});
    let saved = body(
        request(
            &state,
            Method::PUT,
            "/api/library",
            Some(&admin),
            Some(&csrf),
            Some(ORIGIN),
            Some(reference),
        )
        .await,
    )
    .await;
    let offline = root.with_extension("offline");
    fs::rename(&root, &offline).unwrap();
    let catalog = body(
        request(
            &state,
            Method::GET,
            "/api/catalog",
            Some(&admin),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(catalog["unavailable_sources"], 1);
    assert!(catalog["items"].as_array().unwrap().is_empty());
    let library = body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&admin),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(library["unavailable_items"], 1);
    assert_eq!(library["items"].as_array().unwrap().len(), 1);
    assert_eq!(library["items"][0]["available"], false);
    fs::rename(&offline, &root).unwrap();
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                "/api/library",
                Some(&admin),
                None,
                None,
                None
            )
            .await
        )
        .await["items"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    fs::remove_file(root.join("story.epub")).unwrap();
    let missing = body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&admin),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(missing["items"][0]["available"], false);
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("/api/library/{}", saved["file_key"].as_str().unwrap()),
            Some(&admin),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert!(root.join("track.mp3").exists());
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn personal_library_paginates_saved_items_including_unavailable_files(pool: PgPool) {
    seed_admin(&pool).await;
    let state = state(pool.clone());
    let (admin, _, user) = login(&state, "admin", "administrator test passphrase").await;
    let user = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    let root = shared_media_fixture();
    let source = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Paged library',$3)")
        .bind(&source)
        .bind(user)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let paths: Vec<String> = (0..101)
        .map(|index| format!("unavailable-{index}.mp3"))
        .collect();
    let keys: Vec<String> = paths
        .iter()
        .map(|path| crate::catalog::file_key(&source, path))
        .collect();
    sqlx::query("INSERT INTO user_media(user_id,file_key,source_id,relative_path,saved_at) SELECT $1,x.key,$2,x.path,now() FROM unnest($3::text[],$4::text[]) AS x(key,path)").bind(user).bind(&source).bind(keys).bind(paths).execute(&pool).await.unwrap();
    let first = body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&admin),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let second = body(
        request(
            &state,
            Method::GET,
            "/api/library?offset=100",
            Some(&admin),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(first["items"].as_array().unwrap().len(), 100);
    assert_eq!(first["has_more"], true);
    assert_eq!(first["next_offset"], 100);
    assert_eq!(second["items"].as_array().unwrap().len(), 1);
    assert_eq!(second["has_more"], false);
    assert!(first["items"]
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["available"] == false));
    let second_key = second["items"][0]["file_key"].as_str().unwrap();
    assert!(!first["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["file_key"] == second_key));
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/library?offset=-1",
            Some(&admin),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    fs::remove_dir_all(root).unwrap();
}

fn metadata_book_fixture() -> std::path::PathBuf {
    use std::io::Write;
    let root = std::env::temp_dir().join(format!("grimoire-metadata-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let cover = image::RgbImage::from_pixel(12, 18, image::Rgb([110, 70, 150]));
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&cover)
        .unwrap();
    let mut zip = zip::ZipWriter::new(fs::File::create(root.join("original.epub")).unwrap());
    for (name, bytes) in [
        ("META-INF/container.xml", br#"<container xmlns="urn:oasis:names:tc:opendocument:xmlns:container"><rootfiles><rootfile full-path="OPS/package.opf"/></rootfiles></container>"#.as_slice()),
        ("OPS/package.opf", br#"<package xmlns="http://www.idpf.org/2007/opf"><metadata xmlns:dc="http://purl.org/dc/elements/1.1/"><dc:title>Embedded Book</dc:title><dc:creator>Example Author</dc:creator><dc:subject>Science Fiction</dc:subject><meta name="calibre:series" content="Example Series"/><meta name="calibre:series_index" content="2"/></metadata><manifest><item id="cover" href="cover.jpg" properties="cover-image"/></manifest></package>"#.as_slice()),
        ("OPS/cover.jpg", jpeg.as_slice()),
    ] {
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap();
    root
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn metadata_is_owner_managed_and_artwork_respects_revocation(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, owner_user) = register_user(&state, "owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "viewer").await;
    let owner_id = Uuid::parse_str(owner_user["id"].as_str().unwrap()).unwrap();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    let root = metadata_book_fixture();
    let source = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Metadata source',$3)")
        .bind(&source)
        .bind(owner_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let endpoint = format!("/api/sources/{source}/metadata");
    let details = format!("{endpoint}?path=original.epub");
    let key = catalog::file_key(&source, "original.epub");
    let art = format!("/api/metadata/artwork/{key}");
    let saved = body(
        request(
            &state,
            Method::PUT,
            "/api/library",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"source_id":source,"relative_path":"original.epub"})),
        )
        .await,
    )
    .await;
    assert_eq!(saved["file_key"], key);
    sqlx::query(
        "INSERT INTO source_permissions(source_id,user_id,can_discover) VALUES ($1,$2,true)",
    )
    .bind(&source)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        request(
            &state,
            Method::GET,
            &details,
            Some(&viewer),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let extracted = request(
        &state,
        Method::POST,
        &format!("{endpoint}/extract"),
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(json!({"path":"original.epub"})),
    )
    .await;
    assert_eq!(extracted.status(), StatusCode::OK);
    let extracted = body(extracted).await;
    assert_eq!(extracted["metadata"]["fields"]["title"], "Embedded Book");
    assert_eq!(
        extracted["metadata"]["fields"]["collection"],
        "Example Series"
    );
    assert_eq!(
        extracted["metadata"]["fields"]["genres"],
        json!(["science fiction"])
    );
    assert!(extracted["metadata"]["artwork_url"].is_string());
    assert_eq!(
        request(&state, Method::GET, &art, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&state, Method::GET, &art, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query(
        "UPDATE source_permissions SET can_access_media=true WHERE source_id=$1 AND user_id=$2",
    )
    .bind(&source)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .unwrap();
    let public_view = body(
        request(
            &state,
            Method::GET,
            &details,
            Some(&viewer),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(public_view["can_edit"], false);
    assert_eq!(public_view["fields"]["title"], "Embedded Book");
    let image = request(&state, Method::GET, &art, Some(&viewer), None, None, None).await;
    assert_eq!(image.status(), StatusCode::OK);
    assert_eq!(image.headers()["content-type"], "image/jpeg");
    assert_eq!(image.headers()["cache-control"], "private, no-store");
    for (method, url, data) in [
        (
            Method::PUT,
            endpoint.clone(),
            Some(json!({"path":"original.epub","fields":{"title":"Forbidden"}})),
        ),
        (Method::DELETE, details.clone(), None),
        (
            Method::POST,
            format!("{endpoint}/extract"),
            Some(json!({"path":"original.epub"})),
        ),
        (
            Method::POST,
            format!("{endpoint}/search"),
            Some(json!({"path":"original.epub","provider":"openlibrary","query":"Example"})),
        ),
        (
            Method::POST,
            format!("{endpoint}/preview"),
            Some(json!({"path":"original.epub","provider":"openlibrary","id":"OL1W"})),
        ),
        (
            Method::POST,
            format!("{endpoint}/apply"),
            Some(json!({"path":"original.epub","review_token":Uuid::new_v4()})),
        ),
        (Method::POST, format!("{endpoint}/scan"), Some(json!({}))),
        (Method::GET, format!("{endpoint}/job"), None),
    ] {
        assert_eq!(
            request(
                &state,
                method,
                &url,
                Some(&viewer),
                Some(&viewer_csrf),
                Some(ORIGIN),
                data
            )
            .await
            .status(),
            StatusCode::NOT_FOUND,
            "{url}"
        );
    }
    let edited = request(&state,Method::PUT,&endpoint,Some(&owner),Some(&csrf),Some(ORIGIN),Some(json!({"path":"original.epub","fields":{"title":"My corrected title","genres":[" Fantasy ","fantasy"]}}))).await;
    assert_eq!(edited.status(), StatusCode::OK);
    assert_eq!(body(edited).await["fields"]["genres"], json!(["fantasy"]));
    let rescan = body(
        request(
            &state,
            Method::POST,
            &format!("{endpoint}/extract"),
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"path":"original.epub"})),
        )
        .await,
    )
    .await;
    assert_eq!(rescan["metadata"]["fields"]["title"], "My corrected title");
    let library = body(
        request(
            &state,
            Method::GET,
            "/api/library",
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(library["items"][0]["file_key"], key);
    assert_eq!(library["items"][0]["name"], "original.epub");
    assert_eq!(
        library["items"][0]["metadata"]["fields"]["title"],
        "My corrected title"
    );
    assert!(root.join("original.epub").is_file());
    assert!(!root.join("My corrected title.epub").exists());
    let invalid = request(
        &state,
        Method::PUT,
        &endpoint,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(json!({"path":"../original.epub","fields":{}})),
    )
    .await;
    assert!(!invalid.status().is_success());
    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1 AND user_id=$2")
        .bind(&source)
        .bind(viewer_id)
        .execute(&pool)
        .await
        .unwrap();
    for url in [&details, &art] {
        assert_eq!(
            request(&state, Method::GET, url, Some(&viewer), None, None, None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let reset = body(
        request(
            &state,
            Method::DELETE,
            &details,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            None,
        )
        .await,
    )
    .await;
    assert_eq!(reset["fields"]["title"], "Embedded Book");
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn metadata_reviews_are_bound_expiring_single_use_and_jobs_skip_unchanged(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "owner").await;
    let owner_id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    let root = metadata_book_fixture();
    fs::copy(root.join("original.epub"), root.join("other.epub")).unwrap();
    let source = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Reviews',$3)")
        .bind(&source)
        .bind(owner_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let endpoint = format!("/api/sources/{source}/metadata");
    let review = Uuid::new_v4();
    sqlx::query("INSERT INTO metadata_reviews(id,owner_id,source_id,file_key,provider,provider_id,data) VALUES ($1,$2,$3,$4,'openlibrary','OL1W',$5)").bind(review).bind(owner_id).bind(&source).bind(catalog::file_key(&source,"original.epub")).bind(r#"{"title":"Reviewed Book","genres":["fantasy"]}"#).execute(&pool).await.unwrap();
    let apply_url = format!("{endpoint}/apply");
    let apply = |path: &str| json!({"path":path,"review_token":review});
    assert_eq!(
        request(
            &state,
            Method::POST,
            &apply_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(apply("other.epub"))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE metadata_reviews SET expires_at=now()-interval '1 second' WHERE id=$1")
        .bind(review)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &state,
            Method::POST,
            &apply_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(apply("original.epub"))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    sqlx::query("UPDATE metadata_reviews SET expires_at=now()+interval '30 minutes' WHERE id=$1")
        .bind(review)
        .execute(&pool)
        .await
        .unwrap();
    let custom = request(
        &state,
        Method::PUT,
        &endpoint,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(json!({"path":"original.epub","fields":{"title":"Owner correction"}})),
    )
    .await;
    assert_eq!(custom.status(), StatusCode::OK);
    let applied = request(
        &state,
        Method::POST,
        &apply_url,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(apply("original.epub")),
    )
    .await;
    assert_eq!(applied.status(), StatusCode::OK);
    assert_eq!(
        body(applied).await["metadata"]["fields"]["title"],
        "Owner correction"
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            &apply_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(apply("original.epub"))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let reset = request(
        &state,
        Method::DELETE,
        &format!("{endpoint}?path=original.epub"),
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        None,
    )
    .await;
    assert_eq!(body(reset).await["fields"]["title"], "Reviewed Book");
    for expected_skipped in [0, 2] {
        assert_eq!(
            request(
                &state,
                Method::POST,
                &format!("{endpoint}/scan"),
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(json!({}))
            )
            .await
            .status(),
            StatusCode::ACCEPTED
        );
        let mut done = false;
        for _ in 0..100 {
            let job = body(
                request(
                    &state,
                    Method::GET,
                    &format!("{endpoint}/job"),
                    Some(&owner),
                    None,
                    None,
                    None,
                )
                .await,
            )
            .await;
            if job["status"] == "completed" {
                assert_eq!(job["processed"], 2);
                assert_eq!(job["skipped"], expected_skipped);
                done = true;
                break;
            }
            assert_ne!(job["status"], "failed");
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        assert!(done, "background metadata job did not finish");
    }
    let details = body(
        request(
            &state,
            Method::GET,
            &format!("{endpoint}?path=original.epub"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(details["fields"]["title"], "Reviewed Book");
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn arr_connections_are_optional_owner_only_and_match_existing_files_without_writes(
    pool: PgPool,
) {
    use std::sync::Mutex;
    let requests = Arc::new(Mutex::new(Vec::<String>::new()));
    let seen = requests.clone();
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image::RgbImage::from_pixel(
            10,
            10,
            image::Rgb([20, 100, 180]),
        ))
        .unwrap();
    let mock=Router::new().fallback(axum::routing::any(move |request:Request<Body>| {
        let seen=seen.clone();let jpeg=jpeg.clone();
        async move {
            let path=request.uri().path();seen.lock().unwrap().push(format!("{} {path}",request.method()));
            if request.method()!=Method::GET || request.headers().get("x-api-key").and_then(|value|value.to_str().ok())!=Some("synthetic-arr-key") {return (StatusCode::FORBIDDEN,"Forbidden").into_response();}
            let (app,path)=path.trim_start_matches('/').split_once('/').unwrap_or(("",""));
            if app=="redirect" {return (StatusCode::FOUND,[("location","/must-not-be-followed")]).into_response();}
            if path=="api" {return Json(json!({"current":if app=="lidarr"{"v1"}else if app=="sonarr5"{"v5"}else{"v3"}})).into_response();}
            let endpoint=path.splitn(3,'/').nth(2).unwrap_or("");
            let app_name=if app=="sonarr5"{"Sonarr"}else{app};
            if endpoint=="system/status" {return Json(json!({"appName":app_name,"version":"synthetic latest"})).into_response();}
            if endpoint.starts_with("mediacover/") {return ([("content-type","image/jpeg")],jpeg).into_response();}
            let value=match (app_name,endpoint) {
                ("radarr","movie")=>json!([{"id":1,"title":"An Arr Movie","year":2025,"overview":"Movie overview","genres":["Drama"],"path":"/movies/Movie","movieFile":{"id":2,"path":"/movies/Movie/Movie.mkv"},"images":[{"coverType":"poster","url":"/MediaCover/1/poster.jpg"}],"collection":{"title":"Movie Collection"}}]),
                ("sonarr"|"Sonarr","series")=>json!([{"id":3,"title":"Arr Show","overview":"The whole series synopsis","path":"/tv/Show","year":2024,"genres":["Animation"],"images":[{"coverType":"poster","url":"/MediaCover/3/poster.jpg"}]}]),
                ("sonarr"|"Sonarr","episodefile")=>json!([{"id":4,"path":"/tv/Show/Season 01/ep01.mkv"},{"id":14,"relativePath":"Season 01/ep02.mkv"},{"id":24,"relativePath":"Season 01/ep03.mkv"}]),
                ("sonarr"|"Sonarr","episode")=>json!([{"id":5,"episodeFileId":4,"title":"Arr Pilot","seasonNumber":1,"episodeNumber":1,"overview":"Episode overview"},{"id":15,"episodeFileId":14,"title":"Arr Second","seasonNumber":1,"episodeNumber":2,"overview":"Second episode overview"},{"id":25,"episodeFileId":24,"title":"Different group","seasonNumber":1,"episodeNumber":3}]),
                ("lidarr","artist")=>json!([{"id":6,"artistName":"Arr Artist","path":"/music/Artist","genres":["Jazz"]}]),
                ("lidarr","trackfile")=>json!([{"id":7,"albumId":8,"path":"/music/Artist/Album/song.flac"}]),
                ("lidarr","album/8")=>json!({"id":8,"title":"Arr Album","releaseDate":"2023-01-01","genres":[],"images":[{"coverType":"cover","url":"/MediaCover/Albums/8/cover.jpg"}]}),
                ("lidarr","track")=>json!([{"id":9,"trackFileId":7,"title":"Arr Song","absoluteTrackNumber":2,"mediumNumber":1}]),
                _=>return StatusCode::NOT_FOUND.into_response(),
            };Json(value).into_response()
        }
    }));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let address = listener.local_addr().unwrap();
    let server = tokio::spawn(async move { axum::serve(listener, mock).await.unwrap() });
    let base = format!("http://{address}");
    let mut state = state(pool.clone());
    Arc::get_mut(&mut state.metadata).unwrap().arr = metadata::arr::Config::testing(
        ["sonarr", "sonarr5", "radarr", "lidarr", "redirect"]
            .iter()
            .map(|name| format!("{base}/{name}/"))
            .collect(),
    );
    let (owner, csrf, user) = register_user(&state, "arr_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "arr_viewer").await;
    let root = std::env::temp_dir().join(format!("grimoire-arr-{}", Uuid::new_v4()));
    for path in [
        "shows/Show/Season 01/ep01.mkv",
        "shows/Show/Season 01/ep02.mkv",
        "shows/Show/Season 01/ep03.mkv",
        "movies/Movie/Movie.mkv",
        "music/Artist/Album/song.flac",
        "movies/NotInArr.mkv",
    ] {
        let path = root.join(path);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"synthetic media").unwrap();
    }
    let source = Uuid::new_v4().to_string();
    let user_id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Arr test',$3)")
        .bind(&source)
        .bind(user_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES ($1,$2,true,true)").bind(&source).bind(Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap()).execute(&pool).await.unwrap();
    let endpoint = format!("/api/sources/{source}/arr");
    let initial = body(
        request(
            &state,
            Method::GET,
            &endpoint,
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(initial["connections"], json!([]));
    assert!(requests.lock().unwrap().is_empty());
    assert_eq!(
        request(
            &state,
            Method::GET,
            &endpoint,
            Some(&viewer),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    for (app, path, remote, subpath, title) in [
        (
            "sonarr",
            "shows/Show/Season 01/ep01.mkv",
            "/tv",
            "shows",
            "Arr Pilot",
        ),
        (
            "sonarr5",
            "shows/Show/Season 01/ep01.mkv",
            "/tv",
            "shows",
            "Arr Pilot",
        ),
        (
            "radarr",
            "movies/Movie/Movie.mkv",
            "/movies",
            "movies",
            "An Arr Movie",
        ),
        (
            "lidarr",
            "music/Artist/Album/song.flac",
            "/music",
            "music",
            "Arr Song",
        ),
    ] {
        let id = Uuid::new_v4();
        let connection = format!("{endpoint}/{id}");
        let kind = if app == "sonarr5" { "sonarr" } else { app };
        let settings = json!({"name":app,"kind":kind,"base_url":format!("{base}/{app}/"),"remote_root":remote,"source_subpath":subpath,"api_key":"synthetic-arr-key"});
        assert_eq!(
            request(
                &state,
                Method::PUT,
                &connection,
                Some(&viewer),
                Some(&viewer_csrf),
                Some(ORIGIN),
                Some(settings.clone())
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            request(
                &state,
                Method::PUT,
                &connection,
                Some(&owner),
                None,
                Some(ORIGIN),
                Some(settings.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let saved = request(
            &state,
            Method::PUT,
            &connection,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(settings.clone()),
        )
        .await;
        assert_eq!(saved.status(), StatusCode::OK);
        assert!(!body(saved).await.to_string().contains("synthetic-arr-key"));
        let encrypted: Vec<u8> =
            sqlx::query_scalar("SELECT api_key_encrypted FROM arr_connections WHERE id=$1")
                .bind(id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert!(!encrypted
            .windows(17)
            .any(|bytes| bytes == b"synthetic-arr-key"));
        assert_eq!(
            request(
                &state,
                Method::POST,
                &format!("{connection}/test"),
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                None
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            request(
                &state,
                Method::POST,
                &format!("{connection}/test"),
                Some(&viewer),
                Some(&viewer_csrf),
                Some(ORIGIN),
                None
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        let preview_url = format!("{connection}/preview");
        if kind == "radarr" {
            assert_eq!(
                request(
                    &state,
                    Method::POST,
                    &preview_url,
                    Some(&owner),
                    Some(&csrf),
                    Some(ORIGIN),
                    Some(json!({"path":"movies/NotInArr.mkv"}))
                )
                .await
                .status(),
                StatusCode::BAD_REQUEST
            );
        }

        assert_eq!(
            request(
                &state,
                Method::POST,
                &preview_url,
                Some(&viewer),
                Some(&viewer_csrf),
                Some(ORIGIN),
                Some(json!({"path":path}))
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        let preview = request(
            &state,
            Method::POST,
            &preview_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"path":path})),
        )
        .await;
        assert_eq!(preview.status(), StatusCode::OK);
        let preview = body(preview).await;
        assert_eq!(preview["fields"]["title"], title);
        if kind == "lidarr" {
            assert_eq!(preview["fields"]["genres"], json!(["jazz"]));
        }
        let apply_url = format!("/api/sources/{source}/metadata/apply");
        let apply = json!({"path":path,"review_token":preview["review_token"]});
        let applied = request(
            &state,
            Method::POST,
            &apply_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(apply.clone()),
        )
        .await;
        assert_eq!(applied.status(), StatusCode::OK);
        let applied = body(applied).await;
        assert_eq!(applied["metadata"]["fields"]["title"], title);
        assert!(applied["metadata"]["artwork_url"].is_string());
        assert_eq!(applied["warnings"], json!([]));
        assert_eq!(
            request(
                &state,
                Method::POST,
                &apply_url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(apply)
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        if app == "sonarr" {
            let second = "shows/Show/Season 01/ep02.mkv";
            let third = "shows/Show/Season 01/ep03.mkv";
            for (member, collection) in [(second, "Arr Show"), (third, "Other group")] {
                sqlx::query("INSERT INTO media_metadata(file_key,source_id,relative_path,local_data) VALUES($1,$2,$3,$4)")
                    .bind(crate::catalog::file_key(&source, member)).bind(&source).bind(member)
                    .bind(json!({"collection":collection}).to_string()).execute(&pool).await.unwrap();
            }
            let group = applied["metadata"]["group"]["id"].as_str().unwrap();
            let group_url = format!("/api/sources/{source}/groups/{group}");
            let preview_url = format!("{group_url}/arr/{id}/preview");
            let apply_group_url = format!("{group_url}/arr/apply");
            let series = request(
                &state,
                Method::POST,
                &preview_url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(json!({"path":path})),
            )
            .await;
            assert_eq!(series.status(), StatusCode::OK);
            let series = body(series).await;
            assert_eq!(series["synopsis"], "The whole series synopsis");
            assert_eq!(series["episodes"].as_array().unwrap().len(), 2);
            assert_eq!(series["episodes"][1]["path"], second);
            let file_apply = json!({"path":path,"review_token":series["review_token"]});
            assert_eq!(
                request(
                    &state,
                    Method::POST,
                    &apply_url,
                    Some(&owner),
                    Some(&csrf),
                    Some(ORIGIN),
                    Some(file_apply)
                )
                .await
                .status(),
                StatusCode::BAD_REQUEST
            );
            let update = json!({"path":path,"review_token":series["review_token"],"synopsis":true,"episodes":true});
            assert_eq!(
                request(
                    &state,
                    Method::POST,
                    &apply_group_url,
                    Some(&viewer),
                    Some(&viewer_csrf),
                    Some(ORIGIN),
                    Some(update.clone())
                )
                .await
                .status(),
                StatusCode::NOT_FOUND
            );
            let changed = request(
                &state,
                Method::POST,
                &apply_group_url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(update.clone()),
            )
            .await;
            assert_eq!(changed.status(), StatusCode::OK);
            assert_eq!(body(changed).await["episodes_updated"], 2);
            assert_eq!(
                request(
                    &state,
                    Method::POST,
                    &apply_group_url,
                    Some(&owner),
                    Some(&csrf),
                    Some(ORIGIN),
                    Some(update)
                )
                .await
                .status(),
                StatusCode::BAD_REQUEST
            );
            let metadata = |member: &str| {
                format!(
                    "/api/sources/{source}/metadata?{}",
                    reqwest::Url::parse_with_params("http://localhost", &[("path", member)])
                        .unwrap()
                        .query()
                        .unwrap()
                )
            };
            let view = body(
                request(
                    &state,
                    Method::GET,
                    &metadata(second),
                    Some(&viewer),
                    None,
                    None,
                    None,
                )
                .await,
            )
            .await;
            assert_eq!(view["group"]["synopsis"], "The whole series synopsis");
            assert_eq!(view["group"]["id"], group);
            assert_eq!(view["fields"]["description"], "Second episode overview");
            assert_eq!(view["fields"]["title"], "Arr Second");
            let untouched = body(
                request(
                    &state,
                    Method::GET,
                    &metadata(third),
                    Some(&owner),
                    None,
                    None,
                    None,
                )
                .await,
            )
            .await;
            assert_eq!(untouched["provider"], Value::Null);
            let synopsis_url = format!("{group_url}/synopsis");
            let manual = request(
                &state,
                Method::PUT,
                &synopsis_url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(json!({"path":path,"synopsis":"Manual synopsis"})),
            )
            .await;
            assert_eq!(manual.status(), StatusCode::OK);
            let manual = body(manual).await;
            assert_eq!(manual["group"]["synopsis"], "Manual synopsis");
            assert_eq!(manual["group"]["synopsis_provider"], Value::Null);
        }
        let stale = body(
            request(
                &state,
                Method::POST,
                &preview_url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(json!({"path":path})),
            )
            .await,
        )
        .await;
        let mut edited = settings.clone();
        edited["api_key"] = Value::Null;
        edited["enabled"] = json!(false);
        assert_eq!(
            request(
                &state,
                Method::PUT,
                &connection,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(edited)
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            request(
                &state,
                Method::POST,
                &apply_url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(json!({"path":path,"review_token":stale["review_token"]}))
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            request(
                &state,
                Method::DELETE,
                &connection,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                None
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
    }
    let redirect_id = Uuid::new_v4();
    let redirect_url = format!("{endpoint}/{redirect_id}");
    let settings = json!({"name":"Redirect test","kind":"radarr","base_url":format!("{base}/redirect/"),"remote_root":"/movies","api_key":"synthetic-arr-key"});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &redirect_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(settings)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            &format!("{redirect_url}/test"),
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::BAD_GATEWAY
    );
    assert!(!requests
        .lock()
        .unwrap()
        .iter()
        .any(|request| request.contains("must-not-be-followed")));
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &redirect_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let denied = json!({"name":"Bad target","kind":"sonarr","base_url":"http://169.254.169.254/","remote_root":"/tv","api_key":"synthetic-arr-key"});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &format!("{endpoint}/{}", Uuid::new_v4()),
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(denied)
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    let final_list = body(
        request(
            &state,
            Method::GET,
            &endpoint,
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(final_list["connections"], json!([]));
    assert!(requests
        .lock()
        .unwrap()
        .iter()
        .all(|request| request.starts_with("GET ")));
    assert!(root.join("shows/Show/Season 01/ep01.mkv").is_file());
    server.abort();
    fs::remove_dir_all(root).unwrap();
}

async fn group_image_request(
    state: &AppState,
    path: &str,
    cookie: Option<&str>,
    csrf: Option<&str>,
    bytes: Vec<u8>,
) -> Response {
    let mut builder = Request::builder()
        .extension(ConnectInfo(
            "127.0.0.1:12345".parse::<SocketAddr>().unwrap(),
        ))
        .method(Method::POST)
        .uri(path)
        .header("content-type", "image/jpeg")
        .header("origin", ORIGIN);
    if let Some(cookie) = cookie {
        builder = builder.header("cookie", cookie);
    }
    if let Some(csrf) = csrf {
        builder = builder.header("x-csrf-token", csrf);
    }
    app(state.clone())
        .oneshot(builder.body(Body::from(bytes)).unwrap())
        .await
        .unwrap()
}
#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn group_artwork_and_tags_inherit_without_crossing_ownership_or_revocation(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "group_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "group_viewer").await;
    let root = std::env::temp_dir().join(format!("grimoire-groups-{}", Uuid::new_v4()));
    fs::create_dir(&root).unwrap();
    let second_root = std::env::temp_dir().join(format!("grimoire-groups-{}", Uuid::new_v4()));
    fs::create_dir(&second_root).unwrap();
    for file in ["one.mp3", "two.mp3", "Show.S01E01.mkv", "Show.S02E01.mkv"] {
        fs::write(root.join(file), b"test media").unwrap();
    }
    fs::write(second_root.join("one.mp3"), b"test media").unwrap();
    let source = Uuid::new_v4().to_string();
    let second = Uuid::new_v4().to_string();
    let owner_id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    for (source, owner, path) in [
        (&source, owner_id, &root),
        (&second, viewer_id, &second_root),
    ] {
        sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Group source',$3)")
            .bind(source)
            .bind(owner)
            .bind(path.to_str().unwrap())
            .execute(&pool)
            .await
            .unwrap();
    }
    for (source, file) in [
        (&source, "one.mp3"),
        (&source, "two.mp3"),
        (&second, "one.mp3"),
    ] {
        sqlx::query("INSERT INTO media_metadata(file_key,source_id,relative_path,local_data) VALUES ($1,$2,$3,$4)").bind(catalog::file_key(source,file)).bind(source).bind(file).bind(json!({"title":file,"creator":"Artist","collection":"Album","genres":["rock"]}).to_string()).execute(&pool).await.unwrap();
    }
    let detail = |file: &str| format!("/api/sources/{source}/metadata?path={file}");
    let first = body(
        request(
            &state,
            Method::GET,
            &detail("one.mp3"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let group = first["group"]["id"].as_str().unwrap();
    let series_link = format!("/api/media/link?source_id={source}&group={group}");
    let item_link = format!("/api/media/link?source_id={source}&path=one.mp3");
    for url in [&series_link, &item_link] {
        assert_eq!(
            request(&state, Method::GET, url, None, None, None, None)
                .await
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            request(&state, Method::GET, url, Some(&viewer), None, None, None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let linked = body(
        request(
            &state,
            Method::GET,
            &series_link,
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(linked["item"].is_null());
    assert_eq!(linked["members"].as_array().unwrap().len(), 2);
    assert_eq!(linked["label"], "Artist — Album");
    let linked = body(
        request(
            &state,
            Method::GET,
            &item_link,
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(linked["item"]["relative_path"], "one.mp3");
    assert_eq!(linked["members"].as_array().unwrap().len(), 2);
    let endpoint = format!("/api/sources/{source}/groups/{group}");
    let edit = format!("{endpoint}/genres");
    let image = format!("{endpoint}/artwork?path=one.mp3");
    let other = body(
        request(
            &state,
            Method::GET,
            &format!("/api/sources/{second}/metadata?path=one.mp3"),
            Some(&viewer),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_ne!(first["group"]["id"], other["group"]["id"]);
    assert_eq!(
        request(
            &state,
            Method::GET,
            &format!("{endpoint}?path=one.mp3"),
            Some(&viewer),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES ($1,$2,true,true)").bind(&source).bind(viewer_id).execute(&pool).await.unwrap();
    let tags = json!({"path":"one.mp3","genres":[" Ambient ","ambient","Jazz"]});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &edit,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(tags.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &edit,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(tags.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &edit,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(tags)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        group_image_request(
            &state,
            &image,
            Some(&viewer),
            Some(&viewer_csrf),
            b"<svg/>".to_vec()
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        group_image_request(
            &state,
            &image,
            Some(&owner),
            Some(&csrf),
            b"<svg/>".to_vec()
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        group_image_request(
            &state,
            &image,
            Some(&owner),
            Some(&csrf),
            vec![0; 8 * 1024 * 1024 + 1]
        )
        .await
        .status(),
        StatusCode::PAYLOAD_TOO_LARGE
    );
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&image::RgbImage::from_pixel(
            20,
            30,
            image::Rgb([80, 30, 120]),
        ))
        .unwrap();
    assert_eq!(
        group_image_request(&state, &image, None, None, jpeg.clone())
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let uploaded =
        group_image_request(&state, &image, Some(&owner), Some(&csrf), jpeg.clone()).await;
    assert_eq!(uploaded.status(), StatusCode::OK);
    assert_eq!(body(uploaded).await["group"]["has_custom_artwork"], true);
    let art = request(&state, Method::GET, &image, Some(&viewer), None, None, None).await;
    assert_eq!(art.status(), StatusCode::OK);
    assert_eq!(art.headers()["content-type"], "image/jpeg");
    assert_eq!(art.headers()["cache-control"], "private, no-store");
    for file in ["one.mp3", "two.mp3"] {
        let metadata = body(
            request(
                &state,
                Method::GET,
                &detail(file),
                Some(&viewer),
                None,
                None,
                None,
            )
            .await,
        )
        .await;
        assert_eq!(metadata["fields"]["genres"], json!(["ambient", "jazz"]));
        assert_eq!(metadata["fields"]["title"], file);
        assert_eq!(metadata["group"]["can_edit"], false);
        assert!(metadata["artwork_url"].as_str().unwrap().contains(group));
        assert_eq!(metadata["individual_genres"], json!(["rock"]));
    }
    // A newly indexed member inherits the group without rewriting existing files.
    fs::write(root.join("three.mp3"), b"new member").unwrap();
    sqlx::query("INSERT INTO media_metadata(file_key,source_id,relative_path,local_data) VALUES ($1,$2,'three.mp3',$3)").bind(catalog::file_key(&source,"three.mp3")).bind(&source).bind(json!({"title":"New track","creator":"Artist","collection":"Album","genres":["folk"]}).to_string()).execute(&pool).await.unwrap();
    let third = body(
        request(
            &state,
            Method::GET,
            &detail("three.mp3"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(third["fields"]["genres"], json!(["ambient", "jazz"]));
    assert!(third["group"]["has_custom_artwork"].as_bool().unwrap());
    let unchanged = body(
        request(
            &state,
            Method::GET,
            &format!("/api/sources/{second}/metadata?path=one.mp3"),
            Some(&viewer),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(unchanged["fields"]["genres"], json!(["rock"]));
    assert!(unchanged["artwork_url"].is_null());
    // Filename-only episodes share group artwork even without individual DB rows.
    let episode = body(
        request(
            &state,
            Method::GET,
            &detail("Show.S01E01.mkv"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let tv_group = episode["group"]["id"].as_str().unwrap();
    let tv_art = format!("/api/sources/{source}/groups/{tv_group}/artwork?path=Show.S01E01.mkv");
    assert_eq!(
        group_image_request(&state, &tv_art, Some(&owner), Some(&csrf), jpeg)
            .await
            .status(),
        StatusCode::OK
    );
    let episode2 = body(
        request(
            &state,
            Method::GET,
            &detail("Show.S02E01.mkv"),
            Some(&viewer),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(episode2["group"]["id"], tv_group);
    assert_eq!(
        request(
            &state,
            Method::GET,
            episode2["artwork_url"].as_str().unwrap(),
            Some(&viewer),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let listed = body(
        request(
            &state,
            Method::GET,
            &format!("/api/sources/{source}/media"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(listed
        .as_array()
        .unwrap()
        .iter()
        .all(|item| item["metadata"]["group"]["id"].is_string()));
    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1 AND user_id=$2")
        .bind(&source)
        .bind(viewer_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&state, Method::GET, &image, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &format!("{edit}?path=one.mp3"),
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let reset = body(
        request(
            &state,
            Method::GET,
            &detail("two.mp3"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(reset["fields"]["genres"], json!(["rock"]));
    assert_eq!(reset["group"]["has_custom_artwork"], true);
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &image,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&state, Method::GET, &image, Some(&owner), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    // Regrouped files cannot be used as credentials for the previous group's art.
    sqlx::query("UPDATE media_metadata SET local_data=$2 WHERE file_key=$1")
        .bind(catalog::file_key(&source, "one.mp3"))
        .bind(json!({"collection":"Other album","creator":"Artist"}).to_string())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &edit,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"path":"one.mp3","genres":["wrong group"]}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(fs::read(root.join("two.mp3")).unwrap(), b"test media");
    fs::remove_dir_all(root).unwrap();
    fs::remove_dir_all(second_root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn progress_is_private_permission_checked_and_revision_guarded(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "progress_owner").await;
    let (other, other_csrf, _) = register_user(&state, "progress_other").await;
    let root = shared_media_fixture();
    let source = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Progress test',$3)")
        .bind(&source)
        .bind(Uuid::parse_str(user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let url = format!("/api/progress?source_id={source}&relative_path=clip.mp4");
    assert_eq!(
        request(&state, Method::GET, &url, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert!(
        body(request(&state, Method::GET, &url, Some(&owner), None, None, None).await)
            .await
            .is_null()
    );
    let update = json!({"source_id":source,"relative_path":"clip.mp4","position":15.5,"total":100.0,"expected_revision":0});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&other),
            Some(&other_csrf),
            Some(ORIGIN),
            Some(update.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(update.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let saved = request(
        &state,
        Method::PUT,
        "/api/progress",
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(update.clone()),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(body(saved).await["revision"], 1);
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(update)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let read = body(request(&state, Method::GET, &url, Some(&owner), None, None, None).await).await;
    assert_eq!(read["position"], 15.5);
    let continuing = body(
        request(
            &state,
            Method::GET,
            "/api/continue",
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(continuing["items"].as_array().unwrap().len(), 1);
    assert_eq!(continuing["items"][0]["progress"]["position"], 15.5);
    assert!(continuing["items"][0]["saved_at"].is_null());
    let private = body(
        request(
            &state,
            Method::GET,
            "/api/continue",
            Some(&other),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(private["items"], json!([]));

    let other_id: Uuid = sqlx::query_scalar("SELECT id FROM users WHERE username='progress_other'")
        .fetch_one(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES ($1,$2,true,true)").bind(&source).bind(other_id).execute(&pool).await.unwrap();
    assert!(
        body(request(&state, Method::GET, &url, Some(&other), None, None, None).await)
            .await
            .is_null()
    );
    let own_progress = json!({"source_id":source,"relative_path":"clip.mp4","position":35.0,"total":100.0,"expected_revision":0});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&other),
            Some(&other_csrf),
            Some(ORIGIN),
            Some(own_progress)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let own_continue = body(
        request(
            &state,
            Method::GET,
            "/api/continue",
            Some(&other),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(own_continue["items"][0]["progress"]["position"], 35.0);
    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1")
        .bind(&source)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&state, Method::GET, &url, Some(&other), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let revoked = body(
        request(
            &state,
            Method::GET,
            "/api/continue",
            Some(&other),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(revoked["items"], json!([]));
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn logout_other_sessions_requires_password_and_preserves_current_and_other_users(
    pool: PgPool,
) {
    let state = state(pool.clone());
    let (owner, csrf, _) = register_user(&state, "session_owner").await;
    let (second, _, _) = login(&state, "session_owner", "a long shared source passphrase").await;
    let (other, _, _) = register_user(&state, "session_other").await;
    let url = "/api/auth/logout-others";
    assert_eq!(
        request(
            &state,
            Method::POST,
            url,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(json!({"current_password":"a long shared source passphrase"}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"current_password":"incorrect"}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/auth/me",
            Some(&second),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    let result = request(
        &state,
        Method::POST,
        url,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(json!({"current_password":"a long shared source passphrase"})),
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    assert_eq!(body(result).await["revoked"], 1);
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/auth/me",
            Some(&second),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    for cookie in [&owner, &other] {
        assert_eq!(
            request(
                &state,
                Method::GET,
                "/api/auth/me",
                Some(cookie),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn subtitles_require_media_permission_and_reject_unrelated_paths(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, _, owner_user) = register_user(&state, "caption_owner").await;
    let (viewer, _, viewer_user) = register_user(&state, "caption_viewer").await;
    let root = shared_media_fixture();
    std::fs::write(
        root.join("clip.en.srt"),
        "1\n00:00:01,000 --> 00:00:03,000\nHello <script>\n",
    )
    .unwrap();
    std::fs::write(
        root.join("clip.ass"),
        include_str!("../tests/fixtures/styled.ass"),
    )
    .unwrap();
    std::fs::write(root.join("unrelated.srt"), "private").unwrap();
    #[cfg(unix)]
    std::os::unix::fs::symlink(root.join("unrelated.srt"), root.join("clip.link.srt")).unwrap();
    let source = Uuid::new_v4().to_string();
    let owner_id = Uuid::parse_str(owner_user["id"].as_str().unwrap()).unwrap();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Captions',$3)")
        .bind(&source)
        .bind(owner_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let list = format!("/api/sources/{source}/subtitles?path=clip.mp4");
    assert_eq!(
        request(&state, Method::GET, &list, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    sqlx::query(
        "INSERT INTO source_permissions(source_id,user_id,can_discover) VALUES ($1,$2,true)",
    )
    .bind(&source)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        request(&state, Method::GET, &list, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    let listing =
        body(request(&state, Method::GET, &list, Some(&owner), None, None, None).await).await;
    assert_eq!(listing["tracks"].as_array().unwrap().len(), 2);
    let read = format!("/api/sources/{source}/subtitle?video=clip.mp4&path=clip.en.srt");
    let response = request(&state, Method::GET, &read, Some(&owner), None, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let bytes = to_bytes(response.into_body(), 10000).await.unwrap();
    assert!(std::str::from_utf8(&bytes)
        .unwrap()
        .contains("&lt;script&gt;"));
    for path in [
        "unrelated.srt",
        "../clip.en.srt",
        "clip.link.srt",
        "embedded:999",
    ] {
        let url = format!("/api/sources/{source}/subtitle?video=clip.mp4&path={path}");
        assert_eq!(
            request(&state, Method::GET, &url, Some(&owner), None, None, None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let ass_url = format!("/api/sources/{source}/subtitle?video=clip.mp4&path=clip.ass");
    let response = request(
        &state,
        Method::GET,
        &ass_url,
        Some(&owner),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(
        to_bytes(response.into_body(), 10000)
            .await
            .unwrap()
            .as_ref(),
        include_bytes!("../tests/fixtures/styled.ass")
    );
    sqlx::query(
        "UPDATE source_permissions SET can_access_media=true WHERE source_id=$1 AND user_id=$2",
    )
    .bind(&source)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        request(&state, Method::GET, &read, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1")
        .bind(&source)
        .execute(&pool)
        .await
        .unwrap();
    for url in [
        &list,
        &read,
        &ass_url,
        &format!("/api/sources/{source}/subtitle-font?video=clip.mp4&index=2"),
    ] {
        assert_eq!(
            request(&state, Method::GET, url, Some(&viewer), None, None, None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn readiness_checks_database_and_times_out_without_exposing_details(pool: PgPool) {
    let probe_pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(std::time::Duration::from_secs(10))
        .connect_with(pool.connect_options().as_ref().clone())
        .await
        .unwrap();
    let app_state = state(probe_pool.clone());
    let ready = request(
        &app_state,
        Method::GET,
        "/api/ready",
        None,
        None,
        None,
        None,
    )
    .await;
    assert_eq!(ready.status(), StatusCode::OK);
    assert_eq!(ready.headers()["cache-control"], "private, no-store");
    assert_eq!(
        body(ready).await,
        json!({"status":"ready","service":"grimoire-api"})
    );
    let probes = READINESS_PROBES.try_acquire_many(2).unwrap();
    assert_eq!(
        request(
            &app_state,
            Method::GET,
            "/api/ready",
            None,
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    drop(probes);
    let occupied = probe_pool.acquire().await.unwrap();
    let unavailable = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        request(
            &app_state,
            Method::GET,
            "/api/ready",
            None,
            None,
            None,
            None,
        ),
    )
    .await
    .unwrap();
    assert_eq!(unavailable.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        body(unavailable).await,
        json!({"status":"not_ready","service":"grimoire-api"})
    );
    drop(occupied);
    assert_eq!(
        request(
            &app_state,
            Method::GET,
            "/api/ready",
            None,
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
    probe_pool.close().await;
    assert_eq!(
        request(
            &app_state,
            Method::GET,
            "/api/ready",
            None,
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        request(
            &app_state,
            Method::GET,
            "/api/health",
            None,
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::OK
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn metadata_scan_stop_is_owner_scoped_csrf_protected_and_preserves_terminal_jobs(
    pool: PgPool,
) {
    let app_state = state(pool.clone());
    let (owner, csrf, owner_user) = register_user(&app_state, "scan_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&app_state, "scan_viewer").await;
    let source = Uuid::new_v4().to_string();
    let owner_id = Uuid::parse_str(owner_user["id"].as_str().unwrap()).unwrap();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Cancel test','/nonexistent')",
    )
    .bind(&source)
    .bind(owner_id)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES ($1,$2,true,true)").bind(&source).bind(viewer_id).execute(&pool).await.unwrap();
    sqlx::query("UPDATE users SET is_admin=true WHERE id=$1")
        .bind(viewer_id)
        .execute(&pool)
        .await
        .unwrap();
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO metadata_jobs(id,source_id,owner_id,status,total,processed) VALUES ($1,$2,$3,'running',10,3)").bind(id).bind(&source).bind(owner_id).execute(&pool).await.unwrap();
    let url = format!("/api/sources/{source}/metadata/job/{id}/cancel");
    assert_eq!(
        request(
            &app_state,
            Method::POST,
            &url,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &app_state,
            Method::POST,
            &url,
            Some(&owner),
            None,
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let second_source = Uuid::new_v4().to_string();
    sqlx::query(
        "INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Other source','/other')",
    )
    .bind(&second_source)
    .bind(owner_id)
    .execute(&pool)
    .await
    .unwrap();
    let wrong_source = format!("/api/sources/{second_source}/metadata/job/{id}/cancel");
    assert_eq!(
        request(
            &app_state,
            Method::POST,
            &wrong_source,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let wrong_job = format!(
        "/api/sources/{source}/metadata/job/{}/cancel",
        Uuid::new_v4()
    );
    assert_eq!(
        request(
            &app_state,
            Method::POST,
            &wrong_job,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    for _ in 0..2 {
        assert_eq!(
            request(
                &app_state,
                Method::POST,
                &url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                None
            )
            .await
            .status(),
            StatusCode::ACCEPTED
        );
    }
    let job = body(
        request(
            &app_state,
            Method::GET,
            &format!("/api/sources/{source}/metadata/job"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(job["status"], "interrupted");
    assert_eq!(job["processed"], 3);
    assert!(job["message"]
        .as_str()
        .unwrap()
        .contains("Stopped by owner"));
    for terminal in ["completed", "failed", "interrupted"] {
        sqlx::query("UPDATE metadata_jobs SET status=$2,message=NULL WHERE id=$1")
            .bind(id)
            .bind(terminal)
            .execute(&pool)
            .await
            .unwrap();
        assert_eq!(
            request(
                &app_state,
                Method::POST,
                &url,
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                None
            )
            .await
            .status(),
            StatusCode::ACCEPTED
        );
        let status: String = sqlx::query_scalar("SELECT status FROM metadata_jobs WHERE id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(status, terminal);
    }
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn login_cannot_issue_a_session_after_concurrent_password_reset_or_disable(pool: PgPool) {
    let app_state = state(pool.clone());
    let replacement = auth::hash_password("a replacement race test passphrase")
        .unwrap_or_else(|_| panic!("hash"));
    for change in ["password", "disabled"] {
        let name = format!("race_{change}");
        let (_, _, user) = register_user(&app_state, &name).await;
        let id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
        let mut operator = pool.begin().await.unwrap();
        sqlx::query("SELECT id FROM users WHERE id=$1 FOR UPDATE")
            .bind(id)
            .execute(&mut *operator)
            .await
            .unwrap();
        let login_state = app_state.clone();
        let pending = tokio::spawn(async move {
            request(
                &login_state,
                Method::POST,
                "/api/auth/login",
                None,
                None,
                Some(ORIGIN),
                Some(json!({"username":name,"password":"a long shared source passphrase"})),
            )
            .await
        });
        // Observe the actual server transaction waiting after Argon2 verification;
        // this test does not depend on guessing password-hashing timing.
        tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                let waiting:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE datname=current_database() AND wait_event_type='Lock' AND query LIKE '%password_hash=$2%FOR UPDATE%')").fetch_one(&pool).await.unwrap();
                if waiting { break; }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        }).await.expect("Login did not reach guarded session issuance");
        sqlx::query("UPDATE users SET password_hash=CASE WHEN $2 THEN $3 ELSE password_hash END,disabled=$4 WHERE id=$1").bind(id).bind(change=="password").bind(&replacement).bind(change=="disabled").execute(&mut *operator).await.unwrap();
        sqlx::query("DELETE FROM sessions WHERE user_id=$1")
            .bind(id)
            .execute(&mut *operator)
            .await
            .unwrap();
        operator.commit().await.unwrap();
        let response = pending.await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert!(response.headers().get("set-cookie").is_none());
        assert_eq!(
            body(response).await["error"],
            "Invalid username or password."
        );
        let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1")
            .bind(id)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(sessions, 0);
        let successes: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM auth_events WHERE user_id=$1 AND event='login_succeeded'",
        )
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(successes, 0);
    }
    let failures: i64 =
        sqlx::query_scalar("SELECT count(*) FROM auth_events WHERE event='login_failed'")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(failures, 2);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn concurrent_logins_preserve_the_ten_session_limit(pool: PgPool) {
    let app_state = state(pool.clone());
    let (_, _, user) = register_user(&app_state, "parallel_login").await;
    let id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    for index in 0..9 {
        sqlx::query("INSERT INTO sessions(token_hash,user_id,csrf_token) VALUES ($1,$2,'unused')")
            .bind(format!("seed-{index}"))
            .bind(id)
            .execute(&pool)
            .await
            .unwrap();
    }
    let mut logins = tokio::task::JoinSet::new();
    for _ in 0..4 {
        let state = app_state.clone();
        logins.spawn(async move { request(&state,Method::POST,"/api/auth/login",None,None,Some(ORIGIN),Some(json!({"username":"parallel_login","password":"a long shared source passphrase"}))).await });
    }
    while let Some(result) = logins.join_next().await {
        let response = result.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
    }
    let sessions: i64 = sqlx::query_scalar("SELECT count(*) FROM sessions WHERE user_id=$1")
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(sessions, 10);
    let successes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM auth_events WHERE user_id=$1 AND event='login_succeeded'",
    )
    .bind(id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(successes, 4);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn session_activity_is_private_bounded_and_filters_expired_sessions(pool: PgPool) {
    let app_state = state(pool.clone());
    let (owner, csrf, user) = register_user(&app_state, "activity_owner").await;
    let (_, _, _) = login(
        &app_state,
        "activity_owner",
        "a long shared source passphrase",
    )
    .await;
    let (other, _, _) = register_user(&app_state, "activity_other").await;
    let id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE users SET is_admin=true WHERE id=$1")
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    for (key, expiry, idle) in [("expired", -1, 0), ("idle", 3600, 31)] {
        sqlx::query("INSERT INTO sessions(token_hash,user_id,csrf_token,expires_at,last_seen) VALUES ($1,$2,'never_expose_this_csrf',now()+$3::int*interval '1 second',now()-$4::int*interval '1 minute')").bind(key).bind(id).bind(expiry).bind(idle).execute(&pool).await.unwrap();
    }
    let url = "/api/auth/sessions";
    assert_eq!(
        request(&app_state, Method::GET, url, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let response = request(&app_state, Method::GET, url, Some(&owner), None, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let data = body(response).await;
    let summaries = data.as_array().unwrap();
    assert_eq!(summaries.len(), 2);
    assert_eq!(summaries[0]["is_current"], true);
    assert_eq!(
        summaries
            .iter()
            .filter(|summary| summary["is_current"] == true)
            .count(),
        1
    );
    for summary in summaries {
        let keys: std::collections::HashSet<_> = summary
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect();
        assert_eq!(
            keys,
            std::collections::HashSet::from(["created_at", "last_seen", "is_current"])
        );
        assert!(summary["created_at"].as_f64().unwrap() > 0.0);
    }
    let secrets: Vec<(String, String)> =
        sqlx::query_as("SELECT token_hash,csrf_token FROM sessions")
            .fetch_all(&pool)
            .await
            .unwrap();
    for (token, csrf) in secrets {
        assert!(!data.to_string().contains(&token));
        assert!(!data.to_string().contains(&csrf));
    }
    for index in 0..20 {
        sqlx::query(
            "INSERT INTO sessions(token_hash,user_id,csrf_token) VALUES ($1,$2,'extra_csrf')",
        )
        .bind(format!("extra-{index}"))
        .bind(id)
        .execute(&pool)
        .await
        .unwrap();
    }
    let bounded =
        body(request(&app_state, Method::GET, url, Some(&owner), None, None, None).await).await;
    assert_eq!(bounded.as_array().unwrap().len(), 10);
    assert_eq!(bounded[0]["is_current"], true);
    assert_eq!(
        request(
            &app_state,
            Method::POST,
            "/api/auth/logout-others",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"current_password":"a long shared source passphrase"}))
        )
        .await
        .status(),
        StatusCode::OK
    );
    let remaining =
        body(request(&app_state, Method::GET, url, Some(&owner), None, None, None).await).await;
    assert_eq!(remaining.as_array().unwrap().len(), 1);
    let independent =
        body(request(&app_state, Method::GET, url, Some(&other), None, None, None).await).await;
    assert_eq!(independent.as_array().unwrap().len(), 1);
    assert_eq!(independent[0]["is_current"], true);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL and FFmpeg/FFprobe; run with --ignored"]
async fn mkv_audio_selection_transcodes_and_rechecks_permissions(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, owner_user) = register_user(&state, "audio_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "audio_viewer").await;
    let root = shared_media_fixture();
    let original = root.join("multi.mkv");
    let status = tokio::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "color=c=black:s=320x180:d=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=2",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=880:duration=2",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-map",
            "2:a",
            "-c:v",
            "libx264",
            "-threads",
            "1",
            "-c:a",
            "ac3",
            "-metadata:s:a:0",
            "language=eng",
            "-metadata:s:a:1",
            "language=jpn",
            "-y",
        ])
        .arg(&original)
        .status()
        .await
        .unwrap();
    assert!(status.success());
    let unchanged = std::fs::read(&original).unwrap();
    let source = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Audio',$3)")
        .bind(&source)
        .bind(Uuid::parse_str(owner_user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "INSERT INTO source_permissions(source_id,user_id,can_discover) VALUES ($1,$2,true)",
    )
    .bind(&source)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .unwrap();
    let list = format!("/api/sources/{source}/audio-tracks?path=multi.mkv");
    let prepare = format!("/api/sources/{source}/video-audio?path=multi.mkv");
    assert_eq!(
        request(&state, Method::GET, &list, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            &prepare,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(json!({"stream_index":2}))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            &prepare,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(json!({"stream_index":2}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let listed =
        body(request(&state, Method::GET, &list, Some(&owner), None, None, None).await).await;
    assert_eq!(listed.as_array().unwrap().len(), 2);
    assert_eq!(listed[1]["label"], "jpn");
    assert_eq!(listed[1]["codec"], "ac3");
    assert_eq!(
        request(
            &state,
            Method::POST,
            &prepare,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"stream_index":0}))
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let response = request(
        &state,
        Method::POST,
        &prepare,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(json!({"stream_index":2})),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let prepared = body(response).await;
    let read = format!(
        "/api/sources/{source}/video-audio/{}?path=multi.mkv",
        prepared["id"].as_str().unwrap()
    );
    assert_eq!(
        request(&state, Method::GET, &read, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("UPDATE source_permissions SET can_access_media=true WHERE source_id=$1")
        .bind(&source)
        .execute(&pool)
        .await
        .unwrap();
    let response = request(&state, Method::GET, &read, Some(&viewer), None, None, None).await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let mp4 = root.join("converted.mp4");
    std::fs::write(
        &mp4,
        to_bytes(response.into_body(), 1024 * 1024).await.unwrap(),
    )
    .unwrap();
    let probe = tokio::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(&mp4)
        .output()
        .await
        .unwrap();
    let probe: Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = probe["streams"].as_array().unwrap();
    assert_eq!(streams.len(), 2);
    assert_eq!(streams[0]["codec_name"], "h264");
    assert_eq!(streams[1]["codec_name"], "aac");
    // Decode selected track and count upward zero crossings: 880Hz, not the first 440Hz track.
    let pcm = tokio::process::Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(&mp4)
        .args([
            "-map", "0:a:0", "-ac", "1", "-ar", "8000", "-f", "s16le", "pipe:1",
        ])
        .output()
        .await
        .unwrap();
    let samples: Vec<i16> = pcm
        .stdout
        .as_chunks::<2>()
        .0
        .iter()
        .map(|s| i16::from_le_bytes(*s))
        .collect();
    let crossings = samples.windows(2).filter(|s| s[0] <= 0 && s[1] > 0).count();
    assert!(
        (1600..1900).contains(&crossings),
        "wrong track frequency: {crossings}"
    );
    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1")
        .bind(&source)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&state, Method::GET, &read, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(std::fs::read(&original).unwrap(), unchanged);
    std::fs::write(&original, b"replaced").unwrap();
    assert_eq!(
        request(&state, Method::GET, &read, Some(&owner), None, None, None)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL and FFmpeg; run with --ignored"]
async fn transcode_sessions_segments_and_revocation_are_scoped(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "transcode_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "transcode_viewer").await;
    let root = shared_media_fixture();
    let file = root.join("stream.mkv");
    assert!(tokio::process::Command::new("ffmpeg")
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=s=320x180:r=24:d=14",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=14",
            "-map",
            "0:v",
            "-map",
            "1:a",
            "-c:v",
            "libx264",
            "-threads",
            "1",
            "-c:a",
            "flac",
            "-y"
        ])
        .arg(&file)
        .status()
        .await
        .unwrap()
        .success());
    let source = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,'Transcode',$3)")
        .bind(&source)
        .bind(Uuid::parse_str(user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let policy_url = format!("/api/sources/{source}/transcoding");
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                &policy_url,
                Some(&owner),
                None,
                None,
                None
            )
            .await
        )
        .await["encoder"],
        "cpu"
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &policy_url,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(json!({"encoder":"amd"}))
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &policy_url,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(json!({"encoder":"amd"}))
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/transcode",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"source_id":source,"path":"stream.mkv","height":0,"encoder":"amd"}))
        )
        .await
        .status(),
        StatusCode::UNPROCESSABLE_ENTITY
    );
    let qualities = body(
        request(
            &state,
            Method::GET,
            &format!("/api/transcode/qualities?source_id={source}&path=stream.mkv"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(qualities[0]["height"], 180);
    assert_eq!(qualities[0]["bitrate_kbps"], 1000);
    let settings = json!({"source_id":source,"path":"stream.mkv","height":0,"audio":1});
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/transcode",
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(settings.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let result = request(
        &state,
        Method::POST,
        "/api/transcode",
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(settings),
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let started = body(result).await;
    assert_eq!(started["encoder"], "cpu");
    assert_eq!(started["height"], 180);
    let id = started["id"].as_str().unwrap();
    let manifest = format!("/api/transcode/{id}/manifest.m3u8");
    let segment = format!("/api/transcode/{id}/segment?index=1");
    for url in [&manifest, &segment] {
        assert_eq!(
            request(&state, Method::GET, url, Some(&viewer), None, None, None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    let response = request(
        &state,
        Method::GET,
        &manifest,
        Some(&owner),
        None,
        None,
        None,
    )
    .await;
    let playlist = String::from_utf8(
        to_bytes(response.into_body(), 10000)
            .await
            .unwrap()
            .to_vec(),
    )
    .unwrap();
    assert!(playlist.contains("#EXT-X-ENDLIST"));
    assert_eq!(playlist.matches("#EXTINF:").count(), 3);
    let response = request(
        &state,
        Method::GET,
        &segment,
        Some(&owner),
        None,
        None,
        None,
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(response.headers()["cache-control"], "private, no-store");
    let output = root.join("segment.ts");
    std::fs::write(
        &output,
        to_bytes(response.into_body(), 16 * 1024 * 1024)
            .await
            .unwrap(),
    )
    .unwrap();
    let probe = tokio::process::Command::new("ffprobe")
        .args(["-v", "error", "-show_streams", "-of", "json"])
        .arg(&output)
        .output()
        .await
        .unwrap();
    let probe: Value = serde_json::from_slice(&probe.stdout).unwrap();
    let streams = probe["streams"].as_array().unwrap();
    assert_eq!(streams[0]["codec_name"], "h264");
    assert_eq!(streams[1]["codec_name"], "aac");
    let start = streams[0]["start_time"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!((7.3..7.6).contains(&start), "timestamp offset: {start}");
    let audio_start = streams[1]["start_time"]
        .as_str()
        .unwrap()
        .parse::<f64>()
        .unwrap();
    assert!(
        (0.20..0.35).contains(&(start - audio_start)),
        "AAC preroll must precede video: {audio_start} vs {start}"
    );

    // Even a source owner cannot reuse another account's prepared playback session.
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES($1,$2,true,true)").bind(&source).bind(Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap()).execute(&pool).await.unwrap();
    assert_eq!(
        request(
            &state,
            Method::GET,
            &segment,
            Some(&viewer),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &policy_url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(json!({"encoder":"nvidia"}))
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        request(
            &state,
            Method::GET,
            &segment,
            Some(&owner),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    sqlx::query("DELETE FROM sources WHERE id=$1")
        .bind(&source)
        .execute(&pool)
        .await
        .unwrap();
    for url in [&manifest, &segment] {
        assert_eq!(
            request(&state, Method::GET, url, Some(&owner), None, None, None)
                .await
                .status(),
            StatusCode::NOT_FOUND
        );
    }
    std::fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL; run with --ignored"]
async fn group_membership_copies_identity_preserves_item_fields_and_requires_ownership(
    pool: PgPool,
) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "join_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "join_viewer").await;
    let root = shared_media_fixture();
    fs::write(root.join("target.mp3"), b"target").unwrap();
    fs::write(root.join("loose.mp3"), b"unchanged source").unwrap();
    let source = Uuid::new_v4().to_string();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,'Groups',$3)")
        .bind(&source)
        .bind(Uuid::parse_str(user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    for (path, data) in [
        (
            "target.mp3",
            json!({"title":"Target song","creator":"Artist","collection":"Album","year":2020,"genres":["jazz"],"track":1}),
        ),
        (
            "loose.mp3",
            json!({"title":"Keep my title","creator":"Other artist","collection":"Old album","track":7,"disc":2,"description":"Individual notes","genres":["rock"]}),
        ),
    ] {
        sqlx::query("INSERT INTO media_metadata(file_key,source_id,relative_path,local_data) VALUES($1,$2,$3,$4)").bind(catalog::file_key(&source,path)).bind(&source).bind(path).bind(data.to_string()).execute(&pool).await.unwrap();
    }
    let detail = format!("/api/sources/{source}/metadata?path=target.mp3");
    let target =
        body(request(&state, Method::GET, &detail, Some(&owner), None, None, None).await).await;
    let url = format!("/api/sources/{source}/metadata/group-membership");
    let art = "a".repeat(64);
    sqlx::query("INSERT INTO metadata_artwork(id,jpeg) VALUES($1,$2)")
        .bind(&art)
        .bind(b"test poster".to_vec())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO media_groups(id,source_id,genres,artwork) VALUES($1,$2,$3,$4)")
        .bind(target["group"]["id"].as_str().unwrap())
        .bind(&source)
        .bind(json!(["ambient"]).to_string())
        .bind(&art)
        .execute(&pool)
        .await
        .unwrap();
    let payload = json!({"path":"loose.mp3","target_path":"target.mp3","expected_group_id":target["group"]["id"]});
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES($1,$2,true,true)").bind(&source).bind(Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap()).execute(&pool).await.unwrap();
    assert_eq!(
        request(
            &state,
            Method::POST,
            &url,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            &url,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let mut stale = payload.clone();
    stale["expected_group_id"] = json!("stale");
    assert_eq!(
        request(
            &state,
            Method::POST,
            &url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(stale)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let result = request(
        &state,
        Method::POST,
        &url,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(payload.clone()),
    )
    .await;
    assert_eq!(result.status(), StatusCode::OK);
    let result = body(result).await;
    assert_eq!(result["group"]["id"], target["group"]["id"]);
    assert_eq!(result["fields"]["creator"], "Artist");
    assert_eq!(result["fields"]["collection"], "Album");
    assert_eq!(result["fields"]["genres"], json!(["ambient"]));
    assert_eq!(result["group"]["has_custom_artwork"], true);
    assert!(result["artwork_url"]
        .as_str()
        .unwrap()
        .contains(target["group"]["id"].as_str().unwrap()));
    assert_eq!(result["fields"]["title"], "Keep my title");
    assert_eq!(result["fields"]["track"], 7);
    assert_eq!(result["fields"]["disc"], 2);
    assert_eq!(result["fields"]["description"], "Individual notes");
    assert_eq!(
        fs::read(root.join("loose.mp3")).unwrap(),
        b"unchanged source"
    );
    let mut wrong = payload.clone();
    wrong["path"] = json!("clip.mp4");
    assert_eq!(
        request(
            &state,
            Method::POST,
            &url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(wrong)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    let mut traversal = payload;
    traversal["path"] = json!("../loose.mp3");
    assert_eq!(
        request(
            &state,
            Method::POST,
            &url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(traversal)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let audit: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM metadata_audit WHERE source_id=$1 AND action='group_joined'",
    )
    .bind(&source)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit, 1);
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn additional_origins_are_explicit_and_do_not_bypass_csrf(pool: PgPool) {
    let mut state = state(pool);
    const LAN: &str = "http://mechamaru.local:5173";
    Arc::make_mut(&mut state.config)
        .additional_origins
        .push(LAN.into());
    for origin in [
        None,
        Some("http://192.168.1.51:5173"),
        Some("http://192.168.1.50:9999"),
        Some("https://attacker.test"),
    ] {
        assert_eq!(
            request(
                &state,
                Method::POST,
                "/api/auth/login",
                None,
                None,
                origin,
                Some(json!({"username":"nobody","password":"wrong password"}))
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
    }
    for origin in [ORIGIN, LAN] {
        assert_eq!(
            request(
                &state,
                Method::POST,
                "/api/auth/login",
                None,
                None,
                Some(origin),
                Some(json!({"username":"nobody","password":"wrong password"}))
            )
            .await
            .status(),
            StatusCode::UNAUTHORIZED
        );
    }
    let (cookie, _, _) = register_user(&state, "origin_test_user").await;
    assert_eq!(
        request(
            &state,
            Method::POST,
            "/api/auth/logout",
            Some(&cookie),
            None,
            Some(LAN),
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn music_playlists_and_favorites_are_private_csrf_protected_and_revision_guarded(
    pool: PgPool,
) {
    let state = state(pool);
    let (owner, csrf, _) = register_user(&state, "music_owner").await;
    let (viewer, _, _) = register_user(&state, "music_other").await;
    let endpoint = "/api/music/library";
    assert_eq!(
        request(&state, Method::GET, endpoint, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    let library = json!({"favorites":["artist:example"],"playlists":[{"id":Uuid::new_v4(),"name":"My songs","tracks":["a".repeat(64)]}]});
    let payload = json!({"expected_revision":0,"library":library});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            endpoint,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            endpoint,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    let persisted = body(
        request(
            &state,
            Method::GET,
            endpoint,
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(persisted["library"], library);
    assert_eq!(
        body(
            request(
                &state,
                Method::GET,
                endpoint,
                Some(&viewer),
                None,
                None,
                None
            )
            .await
        )
        .await["library"]["playlists"],
        json!([])
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            endpoint,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(payload)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(request(&state,Method::PUT,endpoint,Some(&owner),Some(&csrf),Some(ORIGIN),Some(json!({"expected_revision":1,"library":{"favorites":[],"playlists":[{"id":Uuid::new_v4(),"name":"Invalid","tracks":["../../secret"]}]}}))).await.status(),StatusCode::BAD_REQUEST);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn statistics_use_recorded_activity_deduplicate_units_and_respect_revocation(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "stats_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "stats_viewer").await;
    let root = shared_media_fixture();
    let source = Uuid::new_v4().to_string();
    let owner_id = Uuid::parse_str(user["id"].as_str().unwrap()).unwrap();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,'Stats test',$3)")
        .bind(&source)
        .bind(owner_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    for (path, metadata) in [
        (
            "clip.mp4",
            json!({"episode":1,"collection":"Show","genres":["drama"]}),
        ),
        (
            "story.epub",
            json!({"collection":"Books","genres":["fantasy"]}),
        ),
    ] {
        sqlx::query("INSERT INTO media_metadata(file_key,source_id,relative_path,local_data) VALUES($1,$2,$3,$4)").bind(catalog::file_key(&source,path)).bind(&source).bind(path).bind(metadata.to_string()).execute(&pool).await.unwrap();
    }
    assert_eq!(
        request(
            &state,
            Method::GET,
            "/api/statistics",
            None,
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let empty = body(
        request(
            &state,
            Method::GET,
            "/api/statistics",
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(empty["watch_seconds"].is_null());
    let update = json!({"source_id":source,"relative_path":"clip.mp4","position":900,"total":1000,"expected_revision":0,"played_seconds":20});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(update.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(update)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let update = json!({"source_id":source,"relative_path":"clip.mp4","position":1000,"total":1000,"expected_revision":1,"played_seconds":5,"completed":true});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(update)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let book = json!({"source_id":source,"relative_path":"story.epub","position":0,"total":2,"locator":"one.xhtml","expected_revision":0});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(book)
        )
        .await
        .status(),
        StatusCode::OK
    );
    let continuing = body(
        request(
            &state,
            Method::GET,
            "/api/continue",
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(continuing["items"]
        .as_array()
        .unwrap()
        .iter()
        .any(|item| item["relative_path"] == "story.epub"));
    for (revision, units, done) in [(1, json!([0, 0]), false), (2, json!([0, 1]), true)] {
        let book = json!({"source_id":source,"relative_path":"story.epub","position":1,"total":2,"locator":"two.xhtml","expected_revision":revision,"completed_units":units,"completed":done});
        assert_eq!(
            request(
                &state,
                Method::PUT,
                "/api/progress",
                Some(&owner),
                Some(&csrf),
                Some(ORIGIN),
                Some(book)
            )
            .await
            .status(),
            StatusCode::OK
        );
    }
    let stats = body(
        request(
            &state,
            Method::GET,
            "/api/statistics",
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(stats["watch_seconds"], 25.0);
    assert_eq!(stats["episodes_watched"], 1);
    assert_eq!(stats["reading_units"], 2);
    assert_eq!(stats["books_completed"], 1);
    assert_eq!(stats["reading_active"], 0);
    assert_eq!(stats["watching_genres"][0]["name"], "drama");
    assert_eq!(stats["reading_genres"][0]["name"], "fantasy");
    assert!(stats["series_completed"].is_null());
    let continuing = body(
        request(
            &state,
            Method::GET,
            "/api/continue",
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(continuing["items"], json!([]));
    let private = body(
        request(
            &state,
            Method::GET,
            "/api/statistics",
            Some(&viewer),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(private["watch_seconds"].is_null());
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES($1,$2,true,true)").bind(&source).bind(viewer_id).execute(&pool).await.unwrap();
    let update = json!({"source_id":source,"relative_path":"clip.mp4","position":10,"total":1000,"expected_revision":0,"played_seconds":7});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            "/api/progress",
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(update)
        )
        .await
        .status(),
        StatusCode::OK
    );
    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1 AND user_id=$2")
        .bind(&source)
        .bind(viewer_id)
        .execute(&pool)
        .await
        .unwrap();
    let revoked = body(
        request(
            &state,
            Method::GET,
            "/api/statistics",
            Some(&viewer),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert!(revoked["watch_seconds"].is_null());
    assert_eq!(revoked["watching_genres"], json!([]));
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn personal_series_preferences_are_private_authorized_and_revision_checked(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "series_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "series_viewer").await;
    let root = shared_media_fixture();
    fs::write(root.join("episode.mkv"), b"test").unwrap();
    let source = Uuid::new_v4().to_string();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,'Series',$3)")
        .bind(&source)
        .bind(Uuid::parse_str(user["id"].as_str().unwrap()).unwrap())
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let key = catalog::file_key(&source, "episode.mkv");
    let metadata = json!({"collection":"Series","episode":1,"title":"Episode"});
    sqlx::query("INSERT INTO media_metadata(file_key,source_id,relative_path,local_data) VALUES($1,$2,'episode.mkv',$3)").bind(&key).bind(&source).bind(metadata.to_string()).execute(&pool).await.unwrap();
    let details = body(
        request(
            &state,
            Method::GET,
            &format!("/api/sources/{source}/metadata?path=episode.mkv"),
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let group = details["group"]["id"].as_str().unwrap();
    let url = format!("/api/sources/{source}/groups/{group}/personal");
    let get = format!("{url}?path=episode.mkv");
    let payload =
        json!({"path":"episode.mkv","status":"active","favorite":true,"expected_revision":0});
    assert_eq!(
        request(&state, Method::GET, &get, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&state, Method::GET, &get, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &url,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        body(request(&state, Method::GET, &get, Some(&owner), None, None, None).await).await,
        json!({"status":null,"favorite":false,"revision":0})
    );
    let saved = request(
        &state,
        Method::PUT,
        &url,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(payload.clone()),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(
        body(saved).await,
        json!({"status":"active","favorite":true,"revision":1})
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    let mut update = payload.clone();
    update["status"] = json!("completed");
    update["expected_revision"] = json!(1);
    let updated = request(
        &state,
        Method::PUT,
        &url,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(update.clone()),
    )
    .await;
    assert_eq!(updated.status(), StatusCode::OK);
    assert_eq!(body(updated).await["revision"], 2);
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media) VALUES($1,$2,true,true)").bind(&source).bind(viewer_id).execute(&pool).await.unwrap();
    assert_eq!(
        body(request(&state, Method::GET, &get, Some(&viewer), None, None, None).await).await
            ["revision"],
        0
    );
    // A stale update must never create a record.
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &url,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(update)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &url,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::OK
    );
    let mut invalid = payload.clone();
    invalid["status"] = json!("public");
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &url,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(invalid)
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        body(request(&state, Method::GET, &get, Some(&owner), None, None, None).await).await
            ["status"],
        "completed"
    );
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM media_progress WHERE file_key=$1")
            .bind(&key)
            .fetch_one(&pool)
            .await
            .unwrap(),
        0
    );
    assert_eq!(
        sqlx::query_scalar::<_, String>("SELECT local_data FROM media_metadata WHERE file_key=$1")
            .bind(&key)
            .fetch_one(&pool)
            .await
            .unwrap(),
        metadata.to_string()
    );
    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1 AND user_id=$2")
        .bind(&source)
        .bind(viewer_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&state, Method::GET, &get, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::PUT,
            &url,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    // A link to a group that no longer contains the file cannot expose old preferences.
    sqlx::query("UPDATE media_metadata SET local_data=$2 WHERE file_key=$1")
        .bind(&key)
        .bind(json!({"collection":"Different"}).to_string())
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&state, Method::GET, &get, Some(&owner), None, None, None)
            .await
            .status(),
        StatusCode::BAD_REQUEST
    );
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn ip_access_is_admin_only_allow_over_deny_global_and_recoverable(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, csrf, user) = register_user(&state, "ip_owner").await;
    let (viewer, _, _) = register_user(&state, "ip_viewer").await;
    sqlx::query("UPDATE users SET is_admin=true WHERE id=$1")
        .bind(Uuid::parse_str(user["id"].as_str().unwrap()).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let url = "/api/admin/security/ip";
    assert_eq!(
        request(&state, Method::GET, url, None, None, None, None)
            .await
            .status(),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        request(&state, Method::GET, url, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    let initial =
        body(request(&state, Method::GET, url, Some(&owner), None, None, None).await).await;
    assert_eq!(initial["revision"], 0);
    assert_eq!(initial["policy"]["default_action"], "allow");
    let policy = json!({"default_action":"deny","allow":["127.0.0.1"],"deny":["0.0.0.0/0","::/0"]});
    let payload = json!({"policy":policy,"expected_revision":0});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            url,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(payload.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    let preview = body(
        request(
            &state,
            Method::POST,
            "/api/admin/security/ip/preview",
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(policy),
        )
        .await,
    )
    .await;
    assert_eq!(preview["would_block_you"], false);
    let saved = request(
        &state,
        Method::PUT,
        url,
        Some(&owner),
        Some(&csrf),
        Some(ORIGIN),
        Some(payload.clone()),
    )
    .await;
    assert_eq!(saved.status(), StatusCode::OK);
    assert_eq!(body(saved).await["revision"], 1);
    assert_eq!(
        request(
            &state,
            Method::PUT,
            url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(payload)
        )
        .await
        .status(),
        StatusCode::CONFLICT
    );
    for path in [
        "/api/auth/me",
        "/api/sources/test/book?path=test.cbz",
        "/assets/app.js",
        "/",
    ] {
        let response = app(state.clone())
            .oneshot(
                Request::builder()
                    .uri(path)
                    .extension(ConnectInfo(
                        "198.51.100.4:1234".parse::<SocketAddr>().unwrap(),
                    ))
                    .header("cookie", &owner)
                    .header("x-forwarded-for", "127.0.0.1")
                    .header("x-grimoire-client-ip", "127.0.0.1")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }
    let mut block =
        json!({"policy":{"default_action":"deny","allow":[],"deny":[]},"expected_revision":1});
    assert_eq!(
        request(
            &state,
            Method::PUT,
            url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(block.clone())
        )
        .await
        .status(),
        StatusCode::BAD_REQUEST
    );
    block["acknowledge_lockout"] = json!(true);
    assert_eq!(
        request(
            &state,
            Method::PUT,
            url,
            Some(&owner),
            Some(&csrf),
            Some(ORIGIN),
            Some(block)
        )
        .await
        .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&state, Method::GET, url, Some(&owner), None, None, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    admin::command(&pool, &["reset-ip-access".into()])
        .await
        .unwrap();
    assert_eq!(
        request(&state, Method::GET, url, Some(&owner), None, None, None)
            .await
            .status(),
        StatusCode::OK
    );
    assert_eq!(
        request(&state, Method::GET, url, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(sqlx::query_scalar::<_,i64>("SELECT count(*) FROM auth_events WHERE event IN ('ip_policy_changed','operator_ip_policy_reset')").fetch_one(&pool).await.unwrap(),3);
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn media_comments_and_replies_follow_item_permissions(pool: PgPool) {
    let state = state(pool.clone());
    let (owner, owner_csrf, owner_user) = register_user(&state, "comment_owner").await;
    let (viewer, viewer_csrf, viewer_user) = register_user(&state, "comment_viewer").await;
    let root = shared_media_fixture();
    let source = Uuid::new_v4().to_string();
    let owner_id = Uuid::parse_str(owner_user["id"].as_str().unwrap()).unwrap();
    let viewer_id = Uuid::parse_str(viewer_user["id"].as_str().unwrap()).unwrap();
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES ($1,$2,'Discussion',$3)")
        .bind(&source)
        .bind(owner_id)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO source_permissions(source_id,user_id,can_discover) VALUES($1,$2,true)",
    )
    .bind(&source)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .unwrap();

    let endpoint = "/api/media/comments";
    let list = format!("{endpoint}?source_id={source}&path=clip.mp4");
    let comment = json!({"source_id":source,"path":"clip.mp4","body":" <b>Hello</b> "});
    for (cookie, expected) in [
        (None, StatusCode::UNAUTHORIZED),
        (Some(viewer.as_str()), StatusCode::NOT_FOUND),
    ] {
        assert_eq!(
            request(&state, Method::GET, &list, cookie, None, None, None)
                .await
                .status(),
            expected
        );
    }
    assert_eq!(
        request(
            &state,
            Method::POST,
            endpoint,
            Some(&owner),
            None,
            Some(ORIGIN),
            Some(comment.clone())
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            endpoint,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(comment.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    let posted = request(
        &state,
        Method::POST,
        endpoint,
        Some(&owner),
        Some(&owner_csrf),
        Some(ORIGIN),
        Some(comment),
    )
    .await;
    assert_eq!(posted.status(), StatusCode::CREATED);
    let posted = body(posted).await;
    assert_eq!(posted["body"], "<b>Hello</b>");
    assert_eq!(posted["author"], "comment_owner");

    sqlx::query(
        "UPDATE source_permissions SET can_access_media=true WHERE source_id=$1 AND user_id=$2",
    )
    .bind(&source)
    .bind(viewer_id)
    .execute(&pool)
    .await
    .unwrap();
    let mut reply =
        json!({"source_id":source,"path":"story.epub","body":"A reply","parent_id":posted["id"]});
    assert_eq!(
        request(
            &state,
            Method::POST,
            endpoint,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(reply.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    reply["path"] = json!("clip.mp4");
    let response = request(
        &state,
        Method::POST,
        endpoint,
        Some(&viewer),
        Some(&viewer_csrf),
        Some(ORIGIN),
        Some(reply.clone()),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let response = body(response).await;
    assert_eq!(response["parent_id"], posted["id"]);
    assert_eq!(response["author"], "comment_viewer");
    assert_eq!(response["reply_to"], "comment_owner");

    sqlx::query("INSERT INTO media_comments(source_id,file_key,author_id,body) SELECT $1,$2,$3,'More comments' FROM generate_series(1,29)")
        .bind(&source).bind(catalog::file_key(&source,"clip.mp4")).bind(owner_id).execute(&pool).await.unwrap();
    let first = request(&state, Method::GET, &list, Some(&viewer), None, None, None).await;
    assert_eq!(first.headers()["cache-control"], "private, no-store");
    let first = body(first).await;
    assert_eq!(first["comments"].as_array().unwrap().len(), 30);
    assert_eq!(first["comments"][0], posted);
    assert_eq!(first["comments"][1], response);
    let after = format!("{list}&after={}", first["next"].as_str().unwrap());
    let last =
        body(request(&state, Method::GET, &after, Some(&viewer), None, None, None).await).await;
    assert_eq!(last["comments"].as_array().unwrap().len(), 1);
    assert!(last["next"].is_null());

    let comment_id = posted["id"].as_str().unwrap();
    let remove = format!("{endpoint}/{comment_id}?source_id={source}&path=clip.mp4");
    let wrong_item = format!("{endpoint}/{comment_id}?source_id={source}&path=story.epub");
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &remove,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    sqlx::query("UPDATE users SET is_admin=true WHERE id=$1 OR id=$2")
        .bind(owner_id)
        .bind(viewer_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &remove,
            Some(&owner),
            None,
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &wrong_item,
            Some(&owner),
            Some(&owner_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &remove,
            Some(&owner),
            Some(&owner_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NO_CONTENT
    );
    let moderated =
        body(request(&state, Method::GET, &list, Some(&owner), None, None, None).await).await;
    assert_eq!(moderated["can_moderate"], true);
    assert_eq!(moderated["comments"][0]["removed"], true);
    assert_eq!(moderated["comments"][0]["body"], "[Comment removed]");
    assert_eq!(moderated["comments"][1]["body"], "A reply");
    assert_eq!(moderated["comments"][1]["reply_to"], "Removed comment");
    assert_eq!(
        request(
            &state,
            Method::POST,
            endpoint,
            Some(&owner),
            Some(&owner_csrf),
            Some(ORIGIN),
            Some(reply.clone())
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );

    sqlx::query("DELETE FROM source_permissions WHERE source_id=$1 AND user_id=$2")
        .bind(&source)
        .bind(viewer_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        request(&state, Method::GET, &list, Some(&viewer), None, None, None)
            .await
            .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::POST,
            endpoint,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            Some(reply)
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(
        request(
            &state,
            Method::DELETE,
            &remove,
            Some(&viewer),
            Some(&viewer_csrf),
            Some(ORIGIN),
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = false)]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn source_provider_upgrade_preserves_local_identity_and_grants(pool: PgPool) {
    let all = sqlx::migrate!();
    let baseline = sqlx::migrate::Migrator {
        migrations: std::borrow::Cow::Owned(
            all.iter()
                .filter(|migration| migration.version <= 18)
                .cloned()
                .collect(),
        ),
        ..sqlx::migrate::Migrator::DEFAULT
    };
    baseline.run(&pool).await.unwrap();
    let app_state = state(pool.clone());
    let (owner_cookie, _, owner) = register_user(&app_state, "upgrade_owner").await;
    let (viewer_cookie, _, viewer) = register_user(&app_state, "upgrade_viewer").await;
    let owner = Uuid::parse_str(owner["id"].as_str().unwrap()).unwrap();
    let viewer = Uuid::parse_str(viewer["id"].as_str().unwrap()).unwrap();
    let root = shared_media_fixture();
    let source = "existing-local-source";
    let key = catalog::file_key(source, "clip.mp4");
    sqlx::query("INSERT INTO sources(id,owner_id,name,path) VALUES($1,$2,'Existing',$3)")
        .bind(source)
        .bind(owner)
        .bind(root.to_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media,acquisition_policy) VALUES($1,$2,true,true,'manual_approval')")
        .bind(source).bind(viewer).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO media_progress(user_id,file_key,source_id,relative_path,position,revision) VALUES($1,$2,$3,'clip.mp4',42,1)")
        .bind(viewer).bind(&key).bind(source).execute(&pool).await.unwrap();

    all.run(&pool).await.unwrap();
    all.run(&pool).await.unwrap();
    let node = Uuid::new_v4();
    sqlx::query("INSERT INTO source_nodes(id,owner_id) VALUES($1,$2)")
        .bind(node)
        .bind(owner)
        .execute(&pool)
        .await
        .unwrap();
    let remote = "unpublished-remote-source";
    let insert = "INSERT INTO sources(id,owner_id,name,kind,node_id) VALUES($1,$2,'Remote','remote_agent',$3)";
    assert!(sqlx::query(insert)
        .bind(remote)
        .bind(viewer)
        .bind(node)
        .execute(&pool)
        .await
        .is_err());
    sqlx::query(insert)
        .bind(remote)
        .bind(owner)
        .bind(node)
        .execute(&pool)
        .await
        .unwrap();

    let catalog = body(
        request(
            &app_state,
            Method::GET,
            &format!("/api/sources/{source}/media"),
            Some(&viewer_cookie),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    let clip = catalog
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["relative_path"] == "clip.mp4")
        .unwrap();
    assert_eq!(clip["file_key"], key);
    assert_eq!(clip["progress"]["position"], 42.0);
    let sources = body(
        request(
            &app_state,
            Method::GET,
            "/api/sources",
            Some(&viewer_cookie),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(sources[0]["acquisition_policy"], "manual_approval");
    assert_eq!(
        request(
            &app_state,
            Method::GET,
            &format!("/api/sources/{remote}/media"),
            Some(&owner_cookie),
            None,
            None,
            None
        )
        .await
        .status(),
        StatusCode::NOT_FOUND
    );
    fs::remove_dir_all(root).unwrap();
}

#[sqlx::test(migrations = "./migrations")]
#[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
async fn source_node_enrollment_authenticates_owner_key_and_rejects_replay(pool: PgPool) {
    use grimoire_api::node_protocol::{self, Enrollment, Heartbeat, Rotation};
    use ring::{
        rand::SystemRandom,
        signature::{Ed25519KeyPair, KeyPair},
    };

    let app_state = state(pool.clone());
    let (owner, owner_csrf, user) = register_user(&app_state, "node_owner").await;
    let (other, other_csrf, _) = register_user(&app_state, "node_other").await;
    assert_eq!(user["is_admin"], false);
    let tokens = "/api/nodes/v1/enrollment-tokens";
    let missing_csrf = request(
        &app_state,
        Method::POST,
        tokens,
        Some(&owner),
        None,
        Some(ORIGIN),
        None,
    )
    .await;
    assert_eq!(missing_csrf.status(), StatusCode::FORBIDDEN);
    let issued = request(
        &app_state,
        Method::POST,
        tokens,
        Some(&owner),
        Some(&owner_csrf),
        Some(ORIGIN),
        None,
    )
    .await;
    assert_eq!(issued.status(), StatusCode::CREATED);
    assert_eq!(issued.headers()["cache-control"], "private, no-store");
    let token = body(issued).await["token"].as_str().unwrap().to_owned();
    let key_bytes = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let key = Ed25519KeyPair::from_pkcs8(key_bytes.as_ref()).unwrap();
    let mut enrollment = Enrollment {
        version: node_protocol::VERSION,
        node_id: Uuid::new_v4(),
        public_key: hex::encode(key.public_key().as_ref()),
        label: "Test agent".into(),
        token,
        signature: String::new(),
    };
    enrollment.signature = hex::encode(key.sign(&enrollment.signed_bytes()).as_ref());
    let valid = serde_json::to_value(&enrollment).unwrap();
    let mut forged = valid.clone();
    forged["label"] = json!("Forged label");
    for (message, status) in [
        (forged, StatusCode::UNAUTHORIZED),
        (valid.clone(), StatusCode::CREATED),
        (valid, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            request(
                &app_state,
                Method::POST,
                node_protocol::ENROLL,
                None,
                None,
                None,
                Some(message)
            )
            .await
            .status(),
            status
        );
    }
    let node_list = "/api/nodes/v1";
    let owned = body(
        request(
            &app_state,
            Method::GET,
            node_list,
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(owned[0]["id"], enrollment.node_id.to_string());
    assert_eq!(owned[0]["online"], true);
    let unrelated = body(
        request(
            &app_state,
            Method::GET,
            node_list,
            Some(&other),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(unrelated, json!([]));
    assert!(owned[0].get("public_key").is_none());

    let mut heartbeat = Heartbeat {
        version: node_protocol::VERSION,
        node_id: enrollment.node_id,
        timestamp_ms: node_protocol::now_ms(),
        signature: String::new(),
    };
    heartbeat.signature = hex::encode(key.sign(&heartbeat.signed_bytes()).as_ref());
    let pulse = serde_json::to_value(&heartbeat).unwrap();
    let mut unsupported = pulse.clone();
    unsupported["version"] = json!(node_protocol::VERSION + 1);
    for (message, status) in [
        (unsupported, StatusCode::UPGRADE_REQUIRED),
        (pulse.clone(), StatusCode::NO_CONTENT),
        (pulse, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            request(
                &app_state,
                Method::POST,
                node_protocol::HEARTBEAT,
                None,
                None,
                None,
                Some(message)
            )
            .await
            .status(),
            status
        );
    }

    let replacement = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
    let replacement = Ed25519KeyPair::from_pkcs8(replacement.as_ref()).unwrap();
    let mut rotation = Rotation {
        version: node_protocol::VERSION,
        node_id: enrollment.node_id,
        timestamp_ms: heartbeat.timestamp_ms + 1,
        new_public_key: hex::encode(replacement.public_key().as_ref()),
        signature: String::new(),
        new_signature: String::new(),
    };
    let signed = rotation.signed_bytes();
    rotation.signature = hex::encode(key.sign(&signed).as_ref());
    rotation.new_signature = hex::encode(replacement.sign(&signed).as_ref());
    let valid = serde_json::to_value(rotation).unwrap();
    let mut forged = valid.clone();
    forged["new_signature"] = forged["signature"].clone();
    for (message, expected) in [
        (forged, StatusCode::UNAUTHORIZED),
        (valid.clone(), StatusCode::NO_CONTENT),
        (valid, StatusCode::UNAUTHORIZED),
    ] {
        assert_eq!(
            request(
                &app_state,
                Method::POST,
                node_protocol::ROTATE,
                None,
                None,
                None,
                Some(message)
            )
            .await
            .status(),
            expected
        );
    }
    heartbeat.timestamp_ms += 2;
    for (signer, expected) in [
        (&key, StatusCode::UNAUTHORIZED),
        (&replacement, StatusCode::NO_CONTENT),
    ] {
        heartbeat.signature = hex::encode(signer.sign(&heartbeat.signed_bytes()).as_ref());
        assert_eq!(
            request(
                &app_state,
                Method::POST,
                node_protocol::HEARTBEAT,
                None,
                None,
                None,
                Some(serde_json::to_value(&heartbeat).unwrap())
            )
            .await
            .status(),
            expected
        );
    }

    let node_url = format!("{node_list}/{}", enrollment.node_id);
    for (cookie, csrf, status) in [
        (&other, &other_csrf, StatusCode::NOT_FOUND),
        (&owner, &owner_csrf, StatusCode::NO_CONTENT),
    ] {
        assert_eq!(
            request(
                &app_state,
                Method::DELETE,
                &node_url,
                Some(cookie),
                Some(csrf),
                Some(ORIGIN),
                None
            )
            .await
            .status(),
            status
        );
    }
    heartbeat.timestamp_ms += 1;
    heartbeat.signature = hex::encode(replacement.sign(&heartbeat.signed_bytes()).as_ref());
    assert_eq!(
        request(
            &app_state,
            Method::POST,
            node_protocol::HEARTBEAT,
            None,
            None,
            None,
            Some(serde_json::to_value(heartbeat).unwrap())
        )
        .await
        .status(),
        StatusCode::UNAUTHORIZED
    );
    let owned = body(
        request(
            &app_state,
            Method::GET,
            node_list,
            Some(&owner),
            None,
            None,
            None,
        )
        .await,
    )
    .await;
    assert_eq!(owned[0]["online"], false);
    assert_eq!(owned[0]["revoked"], true);
}
