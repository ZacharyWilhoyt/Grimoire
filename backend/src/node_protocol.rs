//! Shared wire messages for the coordinator and independent Source Agent.
use ring::signature::{UnparsedPublicKey, ED25519};
use serde::{Deserialize, Serialize};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

pub const VERSION: u32 = 1;
pub const CLOCK_SKEW_MS: u64 = 60_000;
pub const ENROLL: &str = "/api/nodes/v1/enroll";
pub const ROTATE: &str = "/api/nodes/v1/rotate";
pub const HEARTBEAT: &str = "/api/nodes/v1/heartbeat";

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Enrollment {
    pub version: u32,
    pub node_id: Uuid,
    pub public_key: String,
    pub token: String,
    pub label: String,
    pub signature: String,
}

impl Enrollment {
    pub fn signed_bytes(&self) -> Vec<u8> {
        format!(
            "grimoire-node/enroll/v{}\n{}\n{}\n{}\n{}",
            self.version, self.node_id, self.public_key, self.token, self.label
        )
        .into_bytes()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Heartbeat {
    pub version: u32,
    pub node_id: Uuid,
    pub timestamp_ms: i64,
    pub signature: String,
}

impl Heartbeat {
    pub fn signed_bytes(&self) -> Vec<u8> {
        format!(
            "grimoire-node/heartbeat/v{}\n{}\n{}",
            self.version, self.node_id, self.timestamp_ms
        )
        .into_bytes()
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Rotation {
    pub version: u32,
    pub node_id: Uuid,
    pub timestamp_ms: i64,
    pub new_public_key: String,
    pub signature: String,
    pub new_signature: String,
}

impl Rotation {
    pub fn signed_bytes(&self) -> Vec<u8> {
        format!(
            "grimoire-node/rotate/v{}\n{}\n{}\n{}",
            self.version, self.node_id, self.timestamp_ms, self.new_public_key
        )
        .into_bytes()
    }
}

pub fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_millis()).ok())
        .unwrap_or(0)
}

pub fn decode_hex<const N: usize>(input: &str) -> Option<[u8; N]> {
    let mut bytes = [0; N];
    hex::decode_to_slice(input, &mut bytes).ok()?;
    Some(bytes)
}

pub fn verify(public_key: &[u8], message: &[u8], signature: &str) -> bool {
    decode_hex::<64>(signature).is_some_and(|signature| {
        UnparsedPublicKey::new(&ED25519, public_key)
            .verify(message, &signature)
            .is_ok()
    })
}
