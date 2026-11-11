//! Outbound-only, bounded byte delivery. Neither side accepts an arbitrary path or URL.
use crate::{auth, nodes, AppError, AppState};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, Request, State},
    http::{header, HeaderMap, Method, StatusCode},
    response::{IntoResponse, Response},
    Extension, Json,
};
use grimoire_api::node_protocol::{self as wire, RelayGrant, SignedRelayGrant};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use std::{
    collections::{HashMap, HashSet},
    io,
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc, Mutex, Weak,
    },
    time::Duration,
};
use tokio::{
    sync::{oneshot, Notify, OwnedSemaphorePermit, Semaphore},
    time::{self, Instant},
};
use uuid::Uuid;

const CHUNK: u64 = wire::RELAY_CHUNK_BYTES as u64;
const PER_STREAM_BYTES_PER_SECOND: u64 = 8 * 1024 * 1024;

struct Pending {
    grant: RelayGrant,
    reply: oneshot::Sender<Bytes>,
    session_hash: String,
    version: Option<Uuid>,
}

pub struct Runtime {
    pending: Mutex<HashMap<Uuid, Pending>>,
    polling: Mutex<HashSet<Uuid>>,
    changed: Notify,
    signing_key: Ed25519KeyPair,
    bytes_delivered: AtomicU64,
    chunks_delivered: AtomicU64,
    total: Arc<Semaphore>,
    nodes: Mutex<HashMap<Uuid, Weak<Semaphore>>>,
    delivery: Mutex<HashMap<Uuid, DeliverySample>>,
}

struct DeliverySample {
    observed: Instant,
    micros_per_mib: u64,
    failed: bool,
}

impl Runtime {
    pub fn new() -> Self {
        let key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
            .expect("relay signing key generation");
        Self {
            pending: Mutex::new(HashMap::new()),
            polling: Mutex::new(HashSet::new()),
            changed: Notify::new(),
            signing_key: Ed25519KeyPair::from_pkcs8(key.as_ref()).expect("relay signing key"),
            bytes_delivered: AtomicU64::new(0),
            chunks_delivered: AtomicU64::new(0),
            total: Arc::new(Semaphore::new(16)),
            nodes: Mutex::new(HashMap::new()),
            delivery: Mutex::new(HashMap::new()),
        }
    }

    pub(crate) fn delivery_health(&self, node: Uuid) -> Option<(u64, bool)> {
        let samples = self.delivery.lock().unwrap();
        let sample = samples
            .get(&node)
            .filter(|sample| sample.observed.elapsed() < Duration::from_secs(120))?;
        Some((sample.micros_per_mib, sample.failed))
    }

    fn observe_delivery(&self, node: Uuid, elapsed: Duration, bytes: Option<u64>) {
        let mut samples = self.delivery.lock().unwrap();
        if samples.len() >= 1024 && !samples.contains_key(&node) {
            samples.retain(|_, sample| sample.observed.elapsed() < Duration::from_secs(120));
            if samples.len() >= 1024 {
                return;
            }
        }
        // Ignore tiny seeks when estimating throughput. Successful reads
        // still clear a recent delivery failure.
        let previous = samples.get(&node).map(|sample| sample.micros_per_mib);
        let measured = bytes
            .filter(|bytes| *bytes >= 64 * 1024)
            .map(|bytes| (elapsed.as_micros() * 1_048_576 / bytes as u128).min(120_000_000) as u64);
        let cost = match (previous, measured) {
            (Some(old), Some(new)) => (old * 3 + new) / 4,
            (old, new) => new.or(old).unwrap_or(120_000_000),
        };
        samples.insert(
            node,
            DeliverySample {
                observed: Instant::now(),
                micros_per_mib: cost,
                failed: bytes.is_none(),
            },
        );
    }

    fn reserve(
        &self,
        node: Uuid,
    ) -> Result<(OwnedSemaphorePermit, OwnedSemaphorePermit), AppError> {
        let global = self.total.clone().try_acquire_owned().map_err(|_| busy())?;
        let mut nodes = self.nodes.lock().unwrap();
        nodes.retain(|_, semaphore| semaphore.strong_count() > 0);
        let semaphore = nodes.get(&node).and_then(Weak::upgrade).unwrap_or_else(|| {
            let semaphore = Arc::new(Semaphore::new(4));
            nodes.insert(node, Arc::downgrade(&semaphore));
            semaphore
        });
        let local = semaphore.try_acquire_owned().map_err(|_| busy())?;
        Ok((global, local))
    }
}

pub async fn stats(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<impl IntoResponse, AppError> {
    crate::security::admin(&identity)?;
    let relay = &state.relay;
    Ok((
        [(header::CACHE_CONTROL, "private, no-store")],
        Json(serde_json::json!({
            "active_streams": 16 - relay.total.available_permits(),
            "chunks_delivered": relay.chunks_delivered.load(Ordering::Relaxed),
            "bytes_delivered": relay.bytes_delivered.load(Ordering::Relaxed),
        })),
    ))
}

fn busy() -> AppError {
    AppError(
        StatusCode::TOO_MANY_REQUESTS,
        "Remote media delivery is busy.".into(),
    )
}

fn missing() -> AppError {
    AppError(StatusCode::NOT_FOUND, "Remote media is unavailable.".into())
}

fn upstream() -> AppError {
    AppError(
        StatusCode::BAD_GATEWAY,
        "The Source Agent did not deliver media in time.".into(),
    )
}

#[derive(sqlx::FromRow)]
struct Target {
    node_id: Uuid,
    owner: bool,
    extension: String,
    size_bytes: i64,
}

async fn authorized(
    state: &AppState,
    viewer: Uuid,
    source: Uuid,
    representation: Uuid,
    session_hash: &str,
    version: Option<Uuid>,
) -> Result<Target, AppError> {
    sqlx::query_as(
        "SELECT n.id AS node_id,s.owner_id=$3 AS owner,r.extension,r.size_bytes
         FROM remote_media_representations r
         JOIN remote_media_locations l ON l.source_id=r.source_id AND l.representation_id=r.id
         JOIN sources s ON s.id=r.source_id AND s.node_id=l.node_id
         JOIN remote_catalog_state c ON c.source_id=s.id
         JOIN source_nodes n ON n.id=l.node_id
         JOIN users u ON u.id=s.owner_id AND NOT u.disabled
         JOIN users viewer ON viewer.id=$3 AND NOT viewer.disabled
         JOIN sessions sess ON sess.user_id=viewer.id AND sess.token_hash=$4
            AND sess.expires_at>now() AND sess.last_seen>now()-interval '30 minutes' 
         LEFT JOIN source_permissions p ON p.source_id=s.id AND p.user_id=$3
         LEFT JOIN remote_source_grants g ON g.source_id=s.id AND g.user_id=$3
         WHERE s.id=$1 AND r.id=$2 AND c.published AND n.revoked_at IS NULL
           AND c.last_confirmed>now()-interval '90 seconds' AND n.last_seen>now()-interval '90 seconds'
           AND ($5::uuid IS NULL OR EXISTS(SELECT 1 FROM catalog_locations v
               WHERE v.source_id=s.id AND v.remote_representation_id=r.id AND v.representation_id=$5))
           AND (s.owner_id=$3 OR (p.can_discover AND p.can_access_media AND g.can_discover AND g.can_access_media))",
    )
    .bind(format!("remote:{source}")).bind(representation).bind(viewer).bind(session_hash).bind(version)
    .fetch_optional(&state.db).await.map_err(|_| auth::unavailable())?.ok_or_else(missing)
}

fn byte_range(headers: &HeaderMap, total: u64) -> Result<(u64, u64, bool), ()> {
    let Some(range) = headers.get(header::RANGE) else {
        return Ok((0, total, false));
    };
    let (start, end) = range
        .to_str()
        .map_err(|_| ())?
        .strip_prefix("bytes=")
        .ok_or(())?
        .split_once('-')
        .ok_or(())?;
    if start.is_empty() {
        let suffix: u64 = end.parse().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok((total.saturating_sub(suffix), total, true));
    }
    let start = start.parse::<u64>().map_err(|_| ())?;
    let end = if end.is_empty() {
        total
    } else {
        end.parse::<u64>()
            .map_err(|_| ())?
            .saturating_add(1)
            .min(total)
    };
    if start >= end {
        return Err(());
    }
    Ok((start, end, true))
}

struct Stream {
    state: AppState,
    viewer: Uuid,
    session_hash: String,
    version: Option<Uuid>,
    source: Uuid,
    representation: Uuid,
    target: Target,
    cursor: u64,
    end: u64,
    next_chunk: Instant,
    _slots: (OwnedSemaphorePermit, OwnedSemaphorePermit),
}

impl Stream {
    async fn chunk(&mut self) -> Result<Option<Bytes>, AppError> {
        if self.cursor == self.end {
            return Ok(None);
        }
        time::sleep_until(self.next_chunk).await;
        let current = authorized(
            &self.state,
            self.viewer,
            self.source,
            self.representation,
            &self.session_hash,
            self.version,
        )
        .await?;
        if current.node_id != self.target.node_id || current.size_bytes != self.target.size_bytes {
            return Err(missing());
        }
        let length = (self.end - self.cursor).min(CHUNK);
        let grant = RelayGrant {
            id: Uuid::new_v4(),
            node_id: current.node_id,
            source_id: self.source,
            representation_id: self.representation,
            viewer_id: self.viewer,
            owner: current.owner,
            offset: self.cursor,
            length,
            size_bytes: current.size_bytes as u64,
            expires_ms: wire::now_ms() + wire::RELAY_GRANT_MS,
        };
        let (reply, receive) = oneshot::channel();
        let relay = &self.state.relay;
        {
            let mut pending = relay.pending.lock().unwrap();
            pending
                .retain(|_, job| !job.reply.is_closed() && job.grant.expires_ms > wire::now_ms());
            pending.insert(
                grant.id,
                Pending {
                    grant,
                    reply,
                    session_hash: self.session_hash.clone(),
                    version: self.version,
                },
            );
        }
        let started = Instant::now();
        relay.changed.notify_waiters();
        let received =
            time::timeout(Duration::from_millis(wire::RELAY_GRANT_MS as u64), receive).await;
        let result = match received {
            Ok(Ok(bytes)) => {
                relay.observe_delivery(
                    current.node_id,
                    started.elapsed(),
                    Some(bytes.len() as u64),
                );
                bytes
            }
            Ok(Err(_)) => return Err(missing()),
            Err(_) => {
                relay.observe_delivery(current.node_id, started.elapsed(), None);
                return Err(upstream());
            }
        };
        self.cursor += length;
        self.next_chunk = Instant::now()
            + Duration::from_secs_f64(length as f64 / PER_STREAM_BYTES_PER_SECOND as f64);
        Ok(Some(result))
    }
}

#[derive(serde::Deserialize)]
pub struct ReadQuery {
    version: Option<Uuid>,
}

pub async fn read(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Path((source, representation)): Path<(Uuid, Uuid)>,
    Query(query): Query<ReadQuery>,
    request: Request,
) -> Result<Response, AppError> {
    let viewer = identity.user.id;
    let target = authorized(
        &state,
        viewer,
        source,
        representation,
        &identity.session_hash,
        query.version,
    )
    .await?;
    let size = target.size_bytes as u64;
    let mime = match target.extension.as_str() {
        "mp4" | "m4v" => "video/mp4",
        "webm" => "video/webm",
        "mkv" => "video/x-matroska",
        "mp3" => "audio/mpeg",
        "m4a" => "audio/mp4",
        "flac" => "audio/flac",
        "aac" => "audio/aac",
        "ogg" | "opus" => "audio/ogg",
        "wav" => "audio/wav",
        "epub" => "application/epub+zip",
        "cbz" => "application/vnd.comicbook+zip",
        _ => "application/octet-stream",
    };
    if (target.extension == "cbz" && size > 4 * 1024_u64.pow(3))
        || (target.extension == "epub" && size > 128 * 1024_u64.pow(2))
    {
        return Err(AppError(
            StatusCode::PAYLOAD_TOO_LARGE,
            "Remote book exceeds its size limit.".into(),
        ));
    }
    let response = Response::builder()
        .header(header::CACHE_CONTROL, "private, no-store")
        .header(header::ACCEPT_RANGES, "bytes")
        .header(header::X_CONTENT_TYPE_OPTIONS, "nosniff");
    let (start, end, partial) = match byte_range(request.headers(), size) {
        Ok(range) => range,
        Err(()) => {
            return Ok(response
                .status(StatusCode::RANGE_NOT_SATISFIABLE)
                .header(header::CONTENT_RANGE, format!("bytes */{size}"))
                .body(Body::empty())
                .expect("valid range response"))
        }
    };
    let mut response = response
        .status(if partial {
            StatusCode::PARTIAL_CONTENT
        } else {
            StatusCode::OK
        })
        .header(header::CONTENT_TYPE, mime)
        .header(header::CONTENT_LENGTH, end - start);
    if partial {
        response = response.header(
            header::CONTENT_RANGE,
            format!("bytes {start}-{}/{size}", end - 1),
        );
    }
    if matches!(target.extension.as_str(), "epub" | "cbz") {
        response = response.header(header::CONTENT_DISPOSITION, "attachment");
    }
    if request.method() == Method::HEAD {
        return Ok(response.body(Body::empty()).expect("valid media response"));
    }
    auth::rate_limit(&state.db, format!("relay:viewer:{viewer}"), 180, 60).await?;
    let slots = state.relay.reserve(target.node_id)?;
    let mut stream = Stream {
        state,
        viewer,
        session_hash: identity.session_hash,
        version: query.version,
        source,
        representation,
        target,
        cursor: start,
        end,
        next_chunk: Instant::now(),
        _slots: slots,
    };
    let first = stream.chunk().await?.ok_or_else(missing)?;
    let rest = futures_util::stream::try_unfold(stream, |mut stream| async move {
        let data = stream
            .chunk()
            .await
            .map_err(|_| io::Error::other("Remote media delivery ended."))?;
        Ok::<_, io::Error>(data.map(|bytes| (bytes, stream)))
    });
    use futures_util::StreamExt;
    let bytes = futures_util::stream::once(async { Ok::<_, io::Error>(first) }).chain(rest);
    Ok(response
        .body(Body::from_stream(bytes))
        .expect("valid media response"))
}

async fn authenticate(
    state: &AppState,
    headers: &HeaderMap,
    operation: &str,
    payload: &str,
) -> Result<Uuid, AppError> {
    let field = |key| {
        headers
            .get(key)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(nodes::rejected)
    };
    let version = field("x-grimoire-node-version")?
        .parse()
        .map_err(|_| nodes::rejected())?;
    nodes::version(version)?;
    let node: Uuid = field("x-grimoire-node-id")?
        .parse()
        .map_err(|_| nodes::rejected())?;
    let timestamp: i64 = field("x-grimoire-node-timestamp")?
        .parse()
        .map_err(|_| nodes::rejected())?;
    let age = wire::now_ms().saturating_sub(timestamp);
    if !(-5_000..=15_000).contains(&age) {
        return Err(nodes::rejected());
    }
    let key = nodes::active_key(state, node).await?;
    if !wire::verify(
        &key,
        &wire::relay_signature_bytes(operation, node, timestamp, payload),
        field("x-grimoire-node-signature")?,
    ) {
        return Err(nodes::rejected());
    }
    Ok(node)
}

struct PollGuard<'a> {
    polling: &'a Mutex<HashSet<Uuid>>,
    node: Uuid,
}
impl Drop for PollGuard<'_> {
    fn drop(&mut self) {
        self.polling.lock().unwrap().remove(&self.node);
    }
}

pub async fn poll(State(state): State<AppState>, headers: HeaderMap) -> Result<Response, AppError> {
    let node = authenticate(&state, &headers, "poll", "").await?;
    if !state.relay.polling.lock().unwrap().insert(node) {
        return Err(busy());
    }
    let _guard = PollGuard {
        polling: &state.relay.polling,
        node,
    };
    auth::rate_limit(&state.db, format!("relay:poll:{node}"), 1600, 60).await?;
    let grant = time::timeout(Duration::from_secs(8), async {
        loop {
            let notified = state.relay.changed.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();
            let pending = state
                .relay
                .pending
                .lock()
                .unwrap()
                .values()
                .filter(|job| {
                    job.grant.node_id == node
                        && !job.reply.is_closed()
                        && job.grant.expires_ms > wire::now_ms()
                })
                .min_by_key(|job| job.grant.expires_ms)
                .map(|job| job.grant.clone());
            if let Some(grant) = pending {
                break grant;
            }
            notified.await;
        }
    })
    .await;
    match grant {
        Ok(grant) => {
            let key = &state.relay.signing_key;
            let signature = hex::encode(key.sign(&grant.signed_bytes()).as_ref());
            Ok((
                [(header::CACHE_CONTROL, "private, no-store")],
                Json(SignedRelayGrant {
                    grant,
                    coordinator_key: hex::encode(key.public_key().as_ref()),
                    signature,
                }),
            )
                .into_response())
        }
        Err(_) => Ok(StatusCode::NO_CONTENT.into_response()),
    }
}

pub async fn upload(
    State(state): State<AppState>,
    Path(job): Path<Uuid>,
    request: Request,
) -> Result<StatusCode, AppError> {
    let digest = request
        .headers()
        .get("x-grimoire-content-digest")
        .and_then(|value| value.to_str().ok())
        .filter(|value| wire::decode_hex::<32>(value).is_some())
        .ok_or_else(nodes::rejected)?
        .to_owned();
    let payload = format!("{job}:{digest}");
    let node = authenticate(&state, request.headers(), "upload", &payload).await?;
    auth::rate_limit(&state.db, format!("relay:upload:{node}"), 1600, 60).await?;
    let (grant, session_hash, version) = {
        let pending = state.relay.pending.lock().unwrap();
        let job = pending
            .get(&job)
            .filter(|job| job.grant.node_id == node && job.grant.expires_ms > wire::now_ms())
            .ok_or_else(missing)?;
        (job.grant.clone(), job.session_hash.clone(), job.version)
    };
    let target = authorized(
        &state,
        grant.viewer_id,
        grant.source_id,
        grant.representation_id,
        &session_hash,
        version,
    )
    .await?;
    if target.node_id != node || target.size_bytes as u64 != grant.size_bytes {
        return Err(missing());
    }
    let job = state
        .relay
        .pending
        .lock()
        .unwrap()
        .remove(&job)
        .ok_or_else(missing)?;
    let remaining = grant.expires_ms.saturating_sub(wire::now_ms());
    if remaining <= 0 || job.reply.is_closed() {
        return Err(missing());
    }
    let bytes = time::timeout(
        Duration::from_millis(remaining as u64),
        axum::body::to_bytes(request.into_body(), grant.length as usize),
    )
    .await
    .map_err(|_| upstream())?
    .map_err(|_| crate::bad_request("Incorrect relay chunk length."))?;
    if wire::body_digest(&bytes) != digest {
        return Err(nodes::rejected());
    }
    if bytes.is_empty() {
        return Ok(StatusCode::NO_CONTENT);
    }
    if bytes.len() as u64 != grant.length {
        return Err(crate::bad_request("Incorrect relay chunk length."));
    }
    let current = authorized(
        &state,
        grant.viewer_id,
        grant.source_id,
        grant.representation_id,
        &session_hash,
        version,
    )
    .await?;
    if current.node_id != node || current.size_bytes as u64 != grant.size_bytes {
        return Err(missing());
    }
    let length = bytes.len() as u64;
    job.reply.send(bytes).map_err(|_| missing())?;
    state
        .relay
        .bytes_delivered
        .fetch_add(length, Ordering::Relaxed);
    state.relay.chunks_delivered.fetch_add(1, Ordering::Relaxed);
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn byte_ranges_cover_seek_suffix_and_invalid_requests() {
        let mut headers = HeaderMap::new();
        assert_eq!(byte_range(&headers, 100), Ok((0, 100, false)));
        for (value, expected) in [
            ("bytes=20-", Ok((20, 100, true))),
            ("bytes=-5", Ok((95, 100, true))),
            ("bytes=0-0", Ok((0, 1, true))),
            ("bytes=90-200", Ok((90, 100, true))),
            ("bytes=0-1,2-3", Err(())),
            ("bytes=100-", Err(())),
            ("bytes=-0", Err(())),
        ] {
            headers.insert(header::RANGE, value.parse().unwrap());
            assert_eq!(byte_range(&headers, 100), expected);
        }
    }
}
