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
fn state(pool: PgPool) -> AppState {
    AppState {
        db: pool,
        config: Arc::new(auth::Config {
            origin: ORIGIN.into(),
            insecure_local: true,
            dummy_hash: auth::hash_password("a dummy password used for tests")
                .unwrap_or_else(|_| panic!("hash")),
        }),
        hashing: Arc::new(tokio::sync::Semaphore::new(4)),
        catalog_scans: Arc::new(tokio::sync::Semaphore::new(2)),
        metadata: Arc::new(metadata::Runtime::new()),
    }
}
async fn request(
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
async fn body(response: Response) -> Value {
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

async fn register_user(state: &AppState, name: &str) -> (String, String, Value) {
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
                ("sonarr"|"Sonarr","series")=>json!([{"id":3,"title":"Arr Show","path":"/tv/Show","year":2024,"genres":["Animation"],"images":[{"coverType":"poster","url":"/MediaCover/3/poster.jpg"}]}]),
                ("sonarr"|"Sonarr","episodefile")=>json!([{"id":4,"path":"/tv/Show/Season 01/ep01.mkv"}]),
                ("sonarr"|"Sonarr","episode")=>json!([{"id":5,"episodeFileId":4,"title":"Arr Pilot","seasonNumber":1,"episodeNumber":1,"overview":"Episode overview"}]),
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
