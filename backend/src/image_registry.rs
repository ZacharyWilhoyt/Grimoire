//! Read-only OCI image distribution from an operator-published filesystem.
use axum::{
    extract::{Path, Request},
    http::{header, Method, StatusCode},
    response::{IntoResponse, Response},
};
use std::path::PathBuf;
use tower_http::services::ServeFile;

const VERSION: &str = "docker-distribution-api-version";

pub async fn ping() -> Response {
    if std::env::var_os("GRIMOIRE_REGISTRY_ROOT").is_none() {
        return StatusCode::NOT_FOUND.into_response();
    }
    ([(VERSION, "registry/2.0")], "{}").into_response()
}

fn digest(reference: &str) -> bool {
    reference.strip_prefix("sha256:").is_some_and(|value| {
        value.len() == 64
            && value
                .bytes()
                .all(|c| c.is_ascii_digit() || (b'a'..=b'f').contains(&c))
    })
}

fn tag(reference: &str) -> bool {
    (1..=128).contains(&reference.len())
        && reference.bytes().enumerate().all(|(index, c)| {
            c.is_ascii_alphanumeric() || c == b'_' || (index > 0 && matches!(c, b'.' | b'-'))
        })
}

pub async fn read(Path((kind, reference)): Path<(String, String)>, request: Request) -> Response {
    let Some(root) = std::env::var_os("GRIMOIRE_REGISTRY_ROOT").map(PathBuf::from) else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let path = root.join(&kind).join(&reference);
    let (mut response, hash) = match kind.as_str() {
        "manifests" if digest(&reference) || tag(&reference) => {
            let Ok(file) = tokio::fs::File::open(&path).await else {
                return StatusCode::NOT_FOUND.into_response();
            };
            use tokio::io::AsyncReadExt;
            let mut bytes = Vec::new();
            if file.take(1_048_577).read_to_end(&mut bytes).await.is_err()
                || bytes.len() > 1_048_576
            {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            }
            let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                return StatusCode::INTERNAL_SERVER_ERROR.into_response();
            };
            let mime = match manifest["mediaType"].as_str() {
                Some(
                    value @ ("application/vnd.oci.image.manifest.v1+json"
                    | "application/vnd.oci.image.index.v1+json"
                    | "application/vnd.docker.distribution.manifest.v2+json"
                    | "application/vnd.docker.distribution.manifest.list.v2+json"),
                ) => value,
                _ => return StatusCode::INTERNAL_SERVER_ERROR.into_response(),
            };
            let hash = format!(
                "sha256:{}",
                grimoire_api::node_protocol::body_digest(&bytes)
            );
            let length = bytes.len();
            let response = Response::builder()
                .header(header::CONTENT_TYPE, mime)
                .header(header::CONTENT_LENGTH, length)
                .body(if request.method() == Method::HEAD {
                    axum::body::Body::empty()
                } else {
                    axum::body::Body::from(bytes)
                })
                .expect("valid manifest response");
            (response, hash)
        }
        "blobs" if digest(&reference) => {
            let Ok(response) = ServeFile::new(path).try_call(request).await else {
                return StatusCode::NOT_FOUND.into_response();
            };
            let mut response = response.map(axum::body::Body::new);
            response.headers_mut().insert(
                header::CONTENT_TYPE,
                "application/octet-stream".parse().unwrap(),
            );
            (response, reference)
        }
        _ => return StatusCode::NOT_FOUND.into_response(),
    };
    response
        .headers_mut()
        .insert(VERSION, "registry/2.0".parse().unwrap());
    if response.status().is_success() {
        response
            .headers_mut()
            .insert("docker-content-digest", hash.parse().unwrap());
    }
    response
}
