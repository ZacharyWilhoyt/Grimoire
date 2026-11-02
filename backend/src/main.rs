use axum::{
    extract::Json,
    http::StatusCode,
    routing::get,
    Router,
};
use serde::Serialize;
use std::net::SocketAddr;
use tower_http::trace::TraceLayer;

#[derive(Serialize)]
struct Health {
    status: &'static str,
    service: &'static str,
}

#[derive(Serialize)]
struct ApiError {
    error: &'static str,
}

async fn health() -> Json<Health> {
    Json(Health {
        status: "ok",
        service: "grimoire-api",
    })
}

async fn not_found() -> (StatusCode, Json<ApiError>) {
    (
        StatusCode::NOT_FOUND,
        Json(ApiError {
            error: "not found",
        }),
    )
}

#[tokio::main]
async fn main() {
    let app = Router::new()
        .route("/api/health", get(health))
        .fallback(not_found)
        .layer(TraceLayer::new_for_http());

    let address = SocketAddr::from(([127, 0, 0, 1], 3000));
    let listener = tokio::net::TcpListener::bind(address)
        .await
        .expect("failed to bind API listener");
    println!("Grimoire API listening on http://{address}");

    axum::serve(listener, app)
        .await
        .expect("API server failed");
}
