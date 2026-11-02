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
