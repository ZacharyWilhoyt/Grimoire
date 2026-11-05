//! Application-wide IP policy. Forwarded identities require a pinned peer AND shared secret.
pub mod tls;
use crate::{auth, AppError, AppState};
use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
    Extension, Json,
};
use ipnet::IpNet;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeSet,
    net::{IpAddr, SocketAddr},
};
use subtle::ConstantTimeEq;

#[derive(Clone, Default)]
pub struct Network {
    pub trusted: Vec<IpNet>,
    pub token: Option<String>,
    pub require_proxy: bool,
}
#[derive(Clone, Copy)]
pub struct ClientIp(pub IpAddr);
impl Network {
    pub fn from_env() -> Result<Self, String> {
        let trusted: Vec<String> = serde_json::from_str(
            &std::env::var("GRIMOIRE_TRUSTED_PROXIES").unwrap_or_else(|_| "[]".into()),
        )
        .map_err(|_| "GRIMOIRE_TRUSTED_PROXIES must be a JSON array of IP/CIDRs.")?;
        let trusted = canonical(trusted)
            .map_err(|_| "Invalid trusted proxy IP/CIDRs.")?
            .iter()
            .map(|value| value.parse().unwrap())
            .collect::<Vec<IpNet>>();
        let token = std::env::var("GRIMOIRE_PROXY_TOKEN").ok();
        if token
            .as_ref()
            .is_some_and(|token| token.len() != 64 || !token.bytes().all(|c| c.is_ascii_hexdigit()))
            || trusted.is_empty() != token.is_none()
        {
            return Err("Trusted proxies require a private 64-hex-character GRIMOIRE_PROXY_TOKEN; configure both together.".into());
        }
        let require_proxy = std::env::var("GRIMOIRE_REQUIRE_PROXY").as_deref() == Ok("true");
        if require_proxy && token.is_none() {
            return Err("GRIMOIRE_REQUIRE_PROXY needs trusted proxies and a token.".into());
        }
        Ok(Self {
            trusted,
            token,
            require_proxy,
        })
    }
    pub fn client(&self, request: &Request) -> Result<IpAddr, AppError> {
        let peer = request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .ok_or_else(auth::unavailable)?
            .0
            .ip()
            .to_canonical();
        if self.trusted.iter().any(|net| net.contains(&peer)) {
            let token = request
                .headers()
                .get("x-grimoire-proxy-token")
                .and_then(|value| value.to_str().ok())
                .unwrap_or("");
            if !self
                .token
                .as_ref()
                .is_some_and(|expected| bool::from(expected.as_bytes().ct_eq(token.as_bytes())))
            {
                return Err(forbidden());
            }
            return request
                .headers()
                .get("x-grimoire-client-ip")
                .and_then(|value| value.to_str().ok())
                .and_then(|value| value.parse::<IpAddr>().ok())
                .map(|ip| ip.to_canonical())
                .ok_or_else(forbidden);
        }
        if self.require_proxy {
            return Err(forbidden());
        }
        // X-Forwarded-For, Forwarded, and untrusted custom headers have no authority.
        Ok(peer)
    }
}
fn forbidden() -> AppError {
    AppError(
        StatusCode::FORBIDDEN,
        "Access from this network is not allowed.".into(),
    )
}
fn canonical(values: Vec<String>) -> Result<Vec<String>, AppError> {
    if values.len() > 128 {
        return Err(crate::bad_request(
            "At most 128 networks are allowed in each list.",
        ));
    }
    let mut result = BTreeSet::new();
    for value in values {
        let net = value
            .trim()
            .parse::<IpNet>()
            .or_else(|_| {
                value
                    .trim()
                    .parse::<IpAddr>()
                    .map(|ip| IpNet::from(ip.to_canonical()))
            })
            .map_err(|_| crate::bad_request("Enter valid IPv4/IPv6 addresses or CIDR networks."))?;
        result.insert(net.trunc().to_string());
    }
    Ok(result.into_iter().collect())
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    #[default]
    Allow,
    Deny,
}
#[derive(Clone, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Policy {
    pub default_action: Action,
    pub allow: Vec<String>,
    pub deny: Vec<String>,
}
impl Policy {
    pub fn validated(self) -> Result<Self, AppError> {
        Ok(Self {
            default_action: self.default_action,
            allow: canonical(self.allow)?,
            deny: canonical(self.deny)?,
        })
    }
    pub fn permits(&self, ip: IpAddr) -> Result<bool, AppError> {
        let matches = |list: &Vec<String>| -> Result<bool, AppError> {
            let nets = list
                .iter()
                .map(|value| value.parse::<IpNet>().map_err(|_| auth::unavailable()))
                .collect::<Result<Vec<_>, _>>()?;
            Ok(nets.iter().any(|net| net.contains(&ip.to_canonical())))
        };
        if matches(&self.allow)? {
            return Ok(true);
        }
        if matches(&self.deny)? {
            return Ok(false);
        }
        Ok(matches!(self.default_action, Action::Allow))
    }
}
#[derive(sqlx::FromRow)]
struct Record {
    ip_revision: i64,
    ip_policy: serde_json::Value,
}
async fn record(db: &sqlx::PgPool) -> Result<(i64, Policy), AppError> {
    let row = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        sqlx::query_as::<_, Record>(
            "SELECT ip_revision,ip_policy FROM instance_security WHERE singleton",
        )
        .fetch_one(db),
    )
    .await
    .map_err(|_| auth::unavailable())?
    .map_err(|_| auth::unavailable())?;
    let policy: Policy = serde_json::from_value(row.ip_policy).map_err(|_| auth::unavailable())?;
    Ok((
        row.ip_revision,
        policy.validated().map_err(|_| auth::unavailable())?,
    ))
}
pub async fn enforce(
    State(state): State<AppState>,
    mut request: Request,
    next: Next,
) -> Result<Response, AppError> {
    let ip = state.network.client(&request)?;
    // Local operational probes expose no application content and must remain usable during DB outages.
    let local_probe = !state.network.require_proxy
        && request
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .is_some_and(|peer| peer.0.ip().is_loopback())
        && matches!(request.uri().path(), "/api/health" | "/api/ready");
    if !local_probe && !record(&state.db).await?.1.permits(ip)? {
        return Err(forbidden());
    }
    request.extensions_mut().insert(ClientIp(ip));
    request.headers_mut().remove("x-grimoire-proxy-token");
    request.headers_mut().remove("x-grimoire-client-ip");
    Ok(next.run(request).await)
}
pub fn admin(identity: &auth::Identity) -> Result<(), AppError> {
    if !identity.user.is_admin {
        return Err(AppError(
            StatusCode::FORBIDDEN,
            "Administrator access required.".into(),
        ));
    }
    Ok(())
}
#[derive(Serialize)]
pub struct View {
    revision: i64,
    policy: Policy,
    current_ip: String,
}
pub async fn read(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Extension(ip): Extension<ClientIp>,
) -> Result<Json<View>, AppError> {
    admin(&identity)?;
    let (revision, policy) = record(&state.db).await?;
    Ok(Json(View {
        revision,
        policy,
        current_ip: ip.0.to_string(),
    }))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    policy: Policy,
    expected_revision: i64,
    #[serde(default)]
    acknowledge_lockout: bool,
}
pub async fn save(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Extension(ip): Extension<ClientIp>,
    Json(body): Json<Update>,
) -> Result<Json<View>, AppError> {
    admin(&identity)?;
    auth::rate_limit(
        &state.db,
        format!("ip-settings:{}", identity.user.id),
        20,
        300,
    )
    .await?;
    if body.expected_revision < 0 || body.expected_revision == i64::MAX {
        return Err(crate::bad_request("Invalid settings revision."));
    }
    let policy = body.policy.validated()?;
    if !policy.permits(ip.0)? && !body.acknowledge_lockout {
        return Err(crate::bad_request("These rules block your current IP. Confirm the lockout warning and ensure you have local recovery access."));
    }
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    let revision:Option<i64>=sqlx::query_scalar("UPDATE instance_security SET ip_policy=$1,ip_revision=ip_revision+1,updated_at=now() WHERE singleton AND ip_revision=$2 RETURNING ip_revision").bind(serde_json::to_value(&policy).map_err(|_|auth::unavailable())?).bind(body.expected_revision).fetch_optional(&mut *tx).await.map_err(|_|auth::unavailable())?;
    let revision = revision.ok_or_else(|| {
        AppError(
            StatusCode::CONFLICT,
            "Settings changed in another window. Reload before saving.".into(),
        )
    })?;
    sqlx::query("INSERT INTO auth_events(user_id,event) VALUES($1,'ip_policy_changed')")
        .bind(identity.user.id)
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    tx.commit().await.map_err(|_| auth::unavailable())?;
    Ok(Json(View {
        revision,
        policy,
        current_ip: ip.0.to_string(),
    }))
}

pub async fn preview(
    Extension(identity): Extension<auth::Identity>,
    Extension(ip): Extension<ClientIp>,
    Json(policy): Json<Policy>,
) -> Result<Json<serde_json::Value>, AppError> {
    admin(&identity)?;
    let policy = policy.validated()?;
    Ok(Json(
        serde_json::json!({"policy":policy,"would_block_you":!policy.permits(ip.0)?,"current_ip":ip.0.to_string()}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn allow_overrides_deny_and_default() {
        let policy = Policy {
            default_action: Action::Deny,
            allow: vec!["192.0.2.4".into(), "2001:db8::/32".into()],
            deny: vec!["0.0.0.0/0".into(), "::/0".into()],
        }
        .validated()
        .unwrap_or_else(|_| panic!("policy"));
        assert!(policy.permits("192.0.2.4".parse().unwrap()).unwrap());
        assert!(policy.permits("::ffff:192.0.2.4".parse().unwrap()).unwrap());
        assert!(policy.permits("2001:db8::1".parse().unwrap()).unwrap());
        assert!(!policy.permits("192.0.2.5".parse().unwrap()).unwrap());
        assert!(canonical(vec!["invalid".into()]).is_err());
    }
    #[test]
    fn forwarding_requires_peer_and_secret() {
        let network = Network {
            trusted: vec!["192.0.2.2/32".parse().unwrap()],
            token: Some("secret".into()),
            require_proxy: false,
        };
        let request = |peer: &str, token: &str, ip: &str| {
            let mut r = Request::builder()
                .header("x-grimoire-proxy-token", token)
                .header("x-grimoire-client-ip", ip)
                .body(axum::body::Body::empty())
                .unwrap();
            r.extensions_mut()
                .insert(ConnectInfo(peer.parse::<SocketAddr>().unwrap()));
            r
        };
        assert_eq!(
            network
                .client(&request("127.0.0.1:123", "secret", "198.51.100.1"))
                .unwrap_or_else(|_| panic!("peer")),
            "127.0.0.1".parse::<IpAddr>().unwrap()
        );
        assert!(network
            .client(&request("192.0.2.2:123", "wrong", "198.51.100.1"))
            .is_err());
        assert_eq!(
            network
                .client(&request("192.0.2.2:123", "secret", "198.51.100.1"))
                .unwrap_or_else(|_| panic!("proxy")),
            "198.51.100.1".parse::<IpAddr>().unwrap()
        );
        assert!(network
            .client(&request(
                "192.0.2.2:123",
                "secret",
                "198.51.100.1, 127.0.0.1"
            ))
            .is_err());
    }
}
