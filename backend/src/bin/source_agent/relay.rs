use super::{catalog, Identity};
use grimoire_api::node_protocol::{self as wire, SignedRelayGrant};
use reqwest::{Client, RequestBuilder, StatusCode, Url};
use ring::signature::Ed25519KeyPair;
use std::{path::Path, time::Duration};

fn signed(
    request: RequestBuilder,
    identity: &Identity,
    key: &Ed25519KeyPair,
    operation: &str,
    payload: &str,
) -> RequestBuilder {
    let timestamp = wire::now_ms();
    let message = wire::relay_signature_bytes(operation, identity.node_id, timestamp, payload);
    request
        .header("x-grimoire-node-version", wire::VERSION.to_string())
        .header("x-grimoire-node-id", identity.node_id.to_string())
        .header("x-grimoire-node-timestamp", timestamp.to_string())
        .header(
            "x-grimoire-node-signature",
            hex::encode(key.sign(&message).as_ref()),
        )
}

async fn next(
    client: &Client,
    url: &Url,
    identity: &Identity,
    key: &Ed25519KeyPair,
) -> Result<Option<SignedRelayGrant>, ()> {
    let mut response = signed(
        client.post(url.join(wire::RELAY_POLL).map_err(|_| ())?),
        identity,
        key,
        "poll",
        "",
    )
    .send()
    .await
    .map_err(|_| ())?;
    match response.status() {
        StatusCode::NO_CONTENT => Ok(None),
        StatusCode::OK => {
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|_| ())? {
                if body.len() + chunk.len() > 4096 {
                    return Err(());
                }
                body.extend_from_slice(&chunk);
            }
            serde_json::from_slice(&body).map(Some).map_err(|_| ())
        }
        _ => Err(()),
    }
}

pub(super) async fn run(
    client: &Client,
    url: &Url,
    identity: &Identity,
    key: &Ed25519KeyPair,
    state: &Path,
    config: &Path,
) {
    loop {
        let message = match next(client, url, identity, key).await {
            Ok(Some(message)) => message,
            Ok(None) => continue,
            Err(()) => {
                tokio::time::sleep(Duration::from_secs(2)).await;
                continue;
            }
        };
        let grant = message.grant;
        let now = wire::now_ms();
        let valid = grant.node_id == identity.node_id
            && grant.expires_ms > now
            && grant.expires_ms <= now.saturating_add(wire::RELAY_GRANT_MS + 5000)
            && wire::decode_hex::<32>(&message.coordinator_key)
                .is_some_and(|key| wire::verify(&key, &grant.signed_bytes(), &message.signature));
        if !valid {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        }
        let job = grant.id;
        let state = state.to_owned();
        let config = config.to_owned();
        let bytes =
            tokio::task::spawn_blocking(move || catalog::read_grant(&state, &config, &grant))
                .await
                .ok()
                .and_then(Result::ok)
                .unwrap_or_default();
        let Ok(endpoint) = url.join(&format!("{}/{job}", wire::RELAY_UPLOAD)) else {
            return;
        };
        let digest = wire::body_digest(&bytes);
        let payload = format!("{job}:{digest}");
        let response = signed(client.post(endpoint), identity, key, "upload", &payload)
            .header("x-grimoire-content-digest", digest)
            .body(bytes)
            .send()
            .await;
        if !response.is_ok_and(|response| response.status() == StatusCode::NO_CONTENT) {
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    }
}
