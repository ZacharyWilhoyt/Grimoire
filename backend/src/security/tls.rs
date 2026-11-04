//! Dedicated Caddy control over a permissioned Unix socket. Never accepts raw proxy config.
use super::admin;
use crate::{auth, AppError, AppState};
use axum::{extract::State, http::StatusCode, Extension, Json};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::BTreeSet, path::PathBuf, time::Duration};
use tokio::io::{AsyncReadExt, AsyncSeekExt};
const LOCK: i64 = 0x4752494d544c53;
const LE: &str = "https://acme-v02.api.letsencrypt.org/directory";
const ISSUER: &str = "acme-v02.api.letsencrypt.org-directory";
#[derive(Default)]
pub struct Runtime {
    pub managed: Option<Managed>,
}
pub struct Managed {
    client: reqwest::Client,
    pub socket: PathBuf,
    pub storage: PathBuf,
    upstream: String,
    proxy_token: String,
}
impl Runtime {
    pub fn from_env() -> Result<Self, String> {
        let Some(socket) = std::env::var_os("GRIMOIRE_CADDY_SOCKET") else {
            return Ok(Self::default());
        };
        let socket = PathBuf::from(socket);
        if !socket.is_absolute() {
            return Err("GRIMOIRE_CADDY_SOCKET must be an absolute Unix socket path.".into());
        }
        let storage = PathBuf::from(
            std::env::var_os("GRIMOIRE_CADDY_STORAGE").unwrap_or_else(|| "/data/caddy".into()),
        );
        let upstream =
            std::env::var("GRIMOIRE_CADDY_UPSTREAM").unwrap_or_else(|_| "api:3000".into());
        if !storage.is_absolute() || storage.to_str().is_none() {
            return Err("Caddy storage needs an absolute UTF-8 path.".into());
        }
        let proxy_token = std::env::var("GRIMOIRE_PROXY_TOKEN")
            .map_err(|_| "Managed Caddy requires GRIMOIRE_PROXY_TOKEN.")?;
        if proxy_token.len() != 64 || !proxy_token.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("Managed Caddy requires a 64-hex-character proxy token.".into());
        }
        let client = reqwest::Client::builder()
            .unix_socket(socket.clone())
            .no_proxy()
            .redirect(reqwest::redirect::Policy::none())
            .timeout(Duration::from_secs(8))
            .build()
            .map_err(|_| "Caddy control client could not start.")?;
        Ok(Self {
            managed: Some(Managed {
                client,
                socket,
                storage,
                upstream,
                proxy_token,
            }),
        })
    }
}
#[derive(Clone, Serialize, Deserialize, Debug, PartialEq)]
#[serde(deny_unknown_fields)]
pub struct Settings {
    pub primary_domain: String,
    pub additional_domains: Vec<String>,
    pub automatic: bool,
}
fn domain(value: &str) -> Result<String, AppError> {
    let value = value.trim().trim_end_matches('.').to_ascii_lowercase();
    if [
        "local",
        "localhost",
        "internal",
        "test",
        "invalid",
        "example",
        "home.arpa",
    ]
    .iter()
    .any(|suffix| value == *suffix || value.ends_with(&format!(".{suffix}")))
    {
        return Err(crate::bad_request("Public TLS requires a public DNS hostname. Local and reserved names such as .local cannot receive Let's Encrypt certificates. Use https://localhost for local setup."));
    }
    if value.len() > 253
        || !value.contains('.')
        || value.parse::<std::net::IpAddr>().is_ok()
        || value.split('.').any(|label| {
            label.is_empty()
                || label.len() > 63
                || label.starts_with('-')
                || label.ends_with('-')
                || !label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
    {
        return Err(crate::bad_request("Use DNS hostnames only, without schemes, paths, ports or wildcards. Use ASCII/punycode for international domains."));
    }
    Ok(value)
}
impl Settings {
    pub fn validated(self) -> Result<Self, AppError> {
        if self.additional_domains.len() > 15 {
            return Err(crate::bad_request("At most 16 TLS domains are supported."));
        }
        let primary_domain = domain(&self.primary_domain)?;
        let mut additional = BTreeSet::new();
        for value in self.additional_domains {
            let value = domain(&value)?;
            if value != primary_domain {
                additional.insert(value);
            }
        }
        Ok(Self {
            primary_domain,
            additional_domains: additional.into_iter().collect(),
            automatic: self.automatic,
        })
    }
    pub fn domains(&self) -> Vec<String> {
        std::iter::once(self.primary_domain.clone())
            .chain(self.additional_domains.clone())
            .collect()
    }
}
#[derive(sqlx::FromRow)]
struct Record {
    tls_revision: i64,
    tls_settings: Option<Value>,
}
async fn record(db: &sqlx::PgPool) -> Result<(i64, Option<Settings>), AppError> {
    let row = sqlx::query_as::<_, Record>(
        "SELECT tls_revision,tls_settings FROM instance_security WHERE singleton",
    )
    .fetch_one(db)
    .await
    .map_err(|_| auth::unavailable())?;
    Ok((
        row.tls_revision,
        row.tls_settings
            .map(serde_json::from_value::<Settings>)
            .transpose()
            .map_err(|_| auth::unavailable())?
            .map(Settings::validated)
            .transpose()
            .map_err(|_| auth::unavailable())?,
    ))
}
pub async fn allows_origin(state: &AppState, origin: &str) -> Result<bool, AppError> {
    if state.tls.managed.is_none() {
        return Ok(false);
    }
    let (_, settings) = record(&state.db).await?;
    Ok(settings.is_some_and(|settings| {
        settings
            .domains()
            .iter()
            .any(|host| origin == format!("https://{host}"))
    }))
}
fn quote(value: &str) -> String {
    serde_json::to_string(value).expect("string serialization")
}
impl Managed {
    fn cert(&self, host: &str, extension: &str) -> PathBuf {
        self.storage
            .join("certificates")
            .join(ISSUER)
            .join(host)
            .join(format!("{host}.{extension}"))
    }
    pub fn caddyfile(&self, settings: &Settings) -> String {
        self.render(settings, false)
    }
    fn bootstrap(&self) -> String {
        self.render(
            &Settings {
                primary_domain: "localhost".into(),
                additional_domains: vec![],
                automatic: true,
            },
            true,
        )
    }
    fn render(&self, settings: &Settings, local: bool) -> String {
        let mut result=format!("{{\n admin {}\n storage file_system {{\n root {}\n }}\n skip_install_trust\n servers {{\n strict_sni_host\n }}\n log default {{\n output file {} {{\n roll_size 10MiB\n roll_keep 2\n }}\n format json\n }}\n",quote(&format!("unix/{}",self.socket.display())),quote(&self.storage.to_string_lossy()),quote(&self.storage.join("grimoire.log").to_string_lossy()));
        if !settings.automatic {
            result.push_str(" auto_https disable_certs\n");
        } else {
            result.push_str(" auto_https ignore_loaded_certs\n");
        }
        result.push_str("}\n");
        for host in settings.domains() {
            result.push_str(&format!("https://{host} {{\n"));
            if local {
                result.push_str(" tls internal\n");
            } else if settings.automatic {
                result.push_str(&format!(" tls {{\n issuer acme {{\n dir {LE}\n }}\n }}\n"));
            } else {
                result.push_str(&format!(
                    " tls {} {}\n",
                    quote(&self.cert(&host, "crt").to_string_lossy()),
                    quote(&self.cert(&host, "key").to_string_lossy())
                ));
            }
            result.push_str(&format!(" reverse_proxy {} {{\n header_up X-Grimoire-Client-IP {{http.request.remote.host}}\n header_up X-Grimoire-Proxy-Token {}\n header_up -Forwarded\n }}\n}}\n",quote(&self.upstream),quote(&self.proxy_token)));
        }
        result
    }
    async fn api(&self, path: &str, body: Option<(Value, &str)>) -> Result<Value, AppError> {
        let request = if let Some((body, content_type)) = body {
            let text = if content_type == "text/caddyfile" {
                body.as_str().unwrap_or_default().to_owned()
            } else {
                body.to_string()
            };
            self.client
                .post(format!("http://localhost{path}"))
                .header("Content-Type", content_type)
                .header("Cache-Control", "must-revalidate")
                .body(text)
        } else {
            self.client.get(format!("http://localhost{path}"))
        };
        let mut response = request.send().await.map_err(|_| {
            AppError(
                StatusCode::SERVICE_UNAVAILABLE,
                "The dedicated Caddy control socket is unavailable. Check the proxy service."
                    .into(),
            )
        })?;
        let success = response.status().is_success();
        let mut bytes = Vec::new();
        while let Some(chunk) = response.chunk().await.map_err(|_| auth::unavailable())? {
            if bytes.len() + chunk.len() > 1024 * 1024 {
                return Err(auth::unavailable());
            }
            bytes.extend_from_slice(&chunk);
        }
        // Caddy errors/config can contain filesystem paths and the proxy token; never forward them.
        if !success {
            return Err(crate::bad_request("Caddy rejected the generated configuration. The last working configuration was retained. Check the private server logs."));
        }
        if bytes.is_empty() {
            Ok(Value::Null)
        } else {
            serde_json::from_slice(&bytes).map_err(|_| auth::unavailable())
        }
    }
    async fn generated_file(&self, file: String) -> Result<Value, AppError> {
        let adapted = self
            .api(
                "/adapt",
                Some((Value::String(file.clone()), "text/caddyfile")),
            )
            .await?;
        let mut config = adapted
            .get("result")
            .filter(|value| value.is_object())
            .cloned()
            .ok_or_else(auth::unavailable)?;
        // Render the token here, not in Caddy: rotating API/Caddy at different times
        // must not tag an old Caddy process's environment as the new configuration.
        use sha2::{Digest, Sha256};
        config["@id"] = Value::String(format!(
            "grimoire-{}",
            hex::encode(Sha256::digest(file.as_bytes()))
        ));
        Ok(config)
    }
    async fn generated(&self, settings: &Settings) -> Result<Value, AppError> {
        self.generated_file(self.caddyfile(settings)).await
    }
    async fn apply_bootstrap(&self, force: bool) -> Result<(), AppError> {
        let config = self.generated_file(self.bootstrap()).await?;
        let current = self.api("/config/", None).await?;
        if force || current.get("@id") != config.get("@id") {
            self.api("/load", Some((config, "application/json")))
                .await?;
        }
        Ok(())
    }
    async fn apply(&self, settings: &Settings) -> Result<Value, AppError> {
        if !settings.automatic {
            for host in settings.domains() {
                if !self.certificate(&host).await.usable {
                    return Err(crate::bad_request("Automatic TLS cannot be disabled until every configured domain has a valid existing certificate. HTTPS was left unchanged."));
                }
            }
        }
        let config = self.generated(settings).await?;
        let previous = self.api("/config/", None).await?;
        if previous.get("@id") != config.get("@id") {
            let applied = async {
                self.api("/load", Some((config.clone(), "application/json")))
                    .await?;
                // Caddy 2.11 can evict a managed cache entry during auto -> file mode.
                // Reprovision after old-app cleanup to ensure the explicit files are cached.
                if !settings.automatic {
                    self.api("/load", Some((config, "application/json")))
                        .await?;
                }
                Ok::<(), AppError>(())
            }
            .await;
            if let Err(error) = applied {
                let _ = self
                    .api("/load", Some((previous.clone(), "application/json")))
                    .await;
                return Err(error);
            }
        }
        Ok(previous)
    }
    async fn certificate(&self, host: &str) -> Certificate {
        let file = self.cert(host, "crt");
        let mut certificate = Certificate {
            domain: host.to_owned(),
            usable: false,
            expires_at: None,
            last_successful_renewal: None,
            error: None,
        };
        if !file.is_file() {
            return certificate;
        }
        if std::fs::metadata(&file).is_ok_and(|meta| meta.len() > 512 * 1024) {
            certificate.error = Some("Certificate file exceeds processing limits.".into());
            return certificate;
        }
        if let Ok(output) = command(
            "openssl",
            &[
                "x509",
                "-in",
                &file.to_string_lossy(),
                "-noout",
                "-enddate",
                "-dateopt",
                "iso_8601",
            ],
        )
        .await
        {
            if output.status.success() {
                certificate.expires_at = String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .find_map(|line| line.strip_prefix("notAfter=").map(str::to_owned));
            }
        }
        certificate.usable = command(
            "openssl",
            &[
                "verify",
                "-partial_chain",
                "-trusted",
                &file.to_string_lossy(),
                "-verify_hostname",
                host,
                &file.to_string_lossy(),
            ],
        )
        .await
        .is_ok_and(|output| output.status.success())
            && self.cert(host, "key").is_file();
        if !certificate.usable {
            certificate.error=Some("The stored certificate is expired, invalid, or does not cover this domain, or its private key is missing.".into());
        }
        certificate
    }
    async fn observe(&self, db: &sqlx::PgPool, domains: &[String]) -> Result<(), AppError> {
        let path = self.storage.join("grimoire.log");
        let Ok(mut file) = tokio::fs::File::open(path).await else {
            return Ok(());
        };
        let length = file
            .metadata()
            .await
            .map_err(|_| auth::unavailable())?
            .len();
        file.seek(std::io::SeekFrom::Start(
            length.saturating_sub(2 * 1024 * 1024),
        ))
        .await
        .map_err(|_| auth::unavailable())?;
        let mut bytes = Vec::new();
        file.take(2 * 1024 * 1024)
            .read_to_end(&mut bytes)
            .await
            .map_err(|_| auth::unavailable())?;
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| line.len() < 32 * 1024)
        {
            let Ok(event) = serde_json::from_slice::<Value>(line) else {
                continue;
            };
            let Some(host) = event["identifier"]
                .as_str()
                .filter(|host| domains.iter().any(|domain| domain == host))
            else {
                continue;
            };
            let Some(ts) = event["ts"].as_f64().filter(|ts| ts.is_finite() && *ts > 0.) else {
                continue;
            };
            let renewed = event["msg"] == "certificate renewed successfully";
            let success = renewed || event["msg"] == "certificate obtained successfully";
            let failed = event["level"] == "error"
                && event["logger"].as_str().is_some_and(|logger| {
                    logger.starts_with("tls.obtain") || logger.starts_with("tls.renew")
                });
            if !success && !failed {
                continue;
            }
            sqlx::query("INSERT INTO tls_certificate_events(domain,last_renewal,last_success,last_error) VALUES($1,$2,$3,$4) ON CONFLICT(domain) DO UPDATE SET last_renewal=GREATEST(tls_certificate_events.last_renewal,EXCLUDED.last_renewal),last_success=GREATEST(tls_certificate_events.last_success,EXCLUDED.last_success),last_error=GREATEST(tls_certificate_events.last_error,EXCLUDED.last_error)").bind(host).bind(renewed.then_some(ts)).bind(success.then_some(ts)).bind(failed.then_some(ts)).execute(db).await.map_err(|_|auth::unavailable())?;
        }
        Ok(())
    }
}
/// Operator recovery must report a control-plane failure instead of merely
/// resetting the DB and implying that the proxy already serves localhost.
pub async fn restore_local_proxy() -> Result<bool, String> {
    let runtime = Runtime::from_env()?;
    let Some(managed) = runtime.managed else {
        return Ok(false);
    };
    managed.apply_bootstrap(true).await.map_err(|error| {
        format!("TLS database settings were reset, but localhost HTTPS could not be applied: {} The supervisor will retry; see deployment recovery instructions.", error.1)
    })?;
    Ok(true)
}
async fn command(binary: &str, args: &[&str]) -> Result<std::process::Output, ()> {
    tokio::time::timeout(
        Duration::from_secs(2),
        tokio::process::Command::new(binary)
            .args(args)
            .kill_on_drop(true)
            .output(),
    )
    .await
    .map_err(|_| ())?
    .map_err(|_| ())
}
#[derive(Serialize)]
struct Certificate {
    domain: String,
    usable: bool,
    expires_at: Option<String>,
    last_successful_renewal: Option<f64>,
    error: Option<String>,
}
#[derive(Serialize)]
pub struct View {
    managed: bool,
    online: bool,
    revision: i64,
    settings: Option<Settings>,
    certificates: Vec<Certificate>,
}
async fn view(state: &AppState) -> Result<View, AppError> {
    let (revision, settings) = record(&state.db).await?;
    let mut certificates = Vec::new();
    let mut online = false;
    if let Some(managed) = &state.tls.managed {
        online = managed.api("/config/", None).await.is_ok();
        if let Some(settings) = &settings {
            managed.observe(&state.db, &settings.domains()).await?;
            for host in settings.domains() {
                let mut cert = managed.certificate(&host).await;
                let events:Option<(Option<f64>,Option<f64>,Option<f64>)>=sqlx::query_as("SELECT last_renewal,last_success,last_error FROM tls_certificate_events WHERE domain=$1").bind(&host).fetch_optional(&state.db).await.map_err(|_|auth::unavailable())?;
                if let Some((renewal, success, error)) = events {
                    cert.last_successful_renewal = renewal;
                    if error.is_some_and(|error| error > success.unwrap_or(0.)) {
                        cert.error=Some("Caddy reported an issuance or renewal failure. Verify DNS and ports 80/443; detailed errors are in the private proxy logs.".into());
                    }
                }
                certificates.push(cert);
            }
        }
    }
    Ok(View {
        managed: state.tls.managed.is_some(),
        online,
        revision,
        settings,
        certificates,
    })
}
pub async fn read(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
) -> Result<Json<View>, AppError> {
    admin(&identity)?;
    Ok(Json(view(&state).await?))
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Update {
    pub settings: Settings,
    pub expected_revision: i64,
}
pub async fn save(
    State(state): State<AppState>,
    Extension(identity): Extension<auth::Identity>,
    Json(body): Json<Update>,
) -> Result<Json<View>, AppError> {
    admin(&identity)?;
    auth::rate_limit(
        &state.db,
        format!("tls-settings:{}", identity.user.id),
        10,
        300,
    )
    .await?;
    let managed =
        state.tls.managed.as_ref().ok_or_else(|| {
            crate::bad_request("Managed Caddy is not configured in this deployment.")
        })?;
    if body.expected_revision < 0 || body.expected_revision == i64::MAX {
        return Err(crate::bad_request("Invalid TLS settings revision."));
    }
    let settings = body.settings.validated()?;
    let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
    sqlx::query("SET LOCAL lock_timeout='3s'")
        .execute(&mut *tx)
        .await
        .map_err(|_| auth::unavailable())?;
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(LOCK)
        .execute(&mut *tx)
        .await
        .map_err(|_| {
            AppError(
                StatusCode::CONFLICT,
                "Another proxy update is in progress. Try again.".into(),
            )
        })?;
    let revision: i64 =
        sqlx::query_scalar("SELECT tls_revision FROM instance_security WHERE singleton FOR UPDATE")
            .fetch_one(&mut *tx)
            .await
            .map_err(|_| auth::unavailable())?;
    if revision != body.expected_revision {
        return Err(AppError(
            StatusCode::CONFLICT,
            "TLS settings changed in another window. Reload before saving.".into(),
        ));
    }
    let previous = managed.apply(&settings).await?;
    let persisted=async{
  sqlx::query("UPDATE instance_security SET tls_settings=$1,tls_revision=tls_revision+1,updated_at=now() WHERE singleton").bind(json!(settings)).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
  sqlx::query("INSERT INTO auth_events(user_id,event) VALUES($1,'tls_settings_changed')").bind(identity.user.id).execute(&mut *tx).await.map_err(|_|auth::unavailable())?;
  tx.commit().await.map_err(|_|auth::unavailable())
 }.await;
    if let Err(error) = persisted {
        let _ = managed
            .api("/load", Some((previous, "application/json")))
            .await;
        return Err(error);
    }
    Ok(Json(view(&state).await?))
}
pub fn supervise(state: AppState) {
    if state.tls.managed.is_none() {
        return;
    }
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            let Some(managed) = &state.tls.managed else {
                return;
            };
            let work = async {
                let mut tx = state.db.begin().await.map_err(|_| auth::unavailable())?;
                let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_xact_lock($1)")
                    .bind(LOCK)
                    .fetch_one(&mut *tx)
                    .await
                    .map_err(|_| auth::unavailable())?;
                if !locked {
                    return Ok(());
                }
                if let Some(settings) = record(&state.db).await?.1 {
                    managed.apply(&settings).await?;
                    managed.observe(&state.db, &settings.domains()).await?;
                } else {
                    managed.apply_bootstrap(false).await?;
                }
                Ok::<(), AppError>(())
            };
            if let Err(error) = work.await {
                eprintln!("Managed Caddy reconciliation unavailable; keeping its last working configuration. {}", error.1);
            }
        }
    });
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn stored_certificate_coverage_and_expiry_are_real() {
        let root = std::env::temp_dir().join(format!("grimoire-cert-{}", uuid::Uuid::new_v4()));
        let managed = Managed {
            client: reqwest::Client::new(),
            socket: root.join("admin.sock"),
            storage: root.clone(),
            upstream: "api:3000".into(),
            proxy_token: "0".repeat(64),
        };
        let cert = managed.cert("video.example.com", "crt");
        let key = managed.cert("video.example.com", "key");
        tokio::fs::create_dir_all(cert.parent().unwrap())
            .await
            .unwrap();
        let output = command(
            "openssl",
            &[
                "req",
                "-x509",
                "-newkey",
                "ec",
                "-pkeyopt",
                "ec_paramgen_curve:P-256",
                "-nodes",
                "-keyout",
                &key.to_string_lossy(),
                "-out",
                &cert.to_string_lossy(),
                "-days",
                "1",
                "-subj",
                "/CN=video.example.com",
                "-addext",
                "subjectAltName=DNS:video.example.com",
            ],
        )
        .await
        .expect("openssl is required for certificate tests");
        assert!(output.status.success());
        let status = managed.certificate("video.example.com").await;
        assert!(status.usable);
        assert!(status.expires_at.is_some());
        assert!(status.last_successful_renewal.is_none());
        tokio::fs::remove_file(key).await.unwrap();
        assert!(!managed.certificate("video.example.com").await.usable);
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
    #[test]
    fn domains_are_strict_canonical_and_deduplicated() {
        let settings = Settings {
            primary_domain: " Example.COM. ".into(),
            additional_domains: vec![
                "example.com".into(),
                "A.example.com".into(),
                "a.example.com".into(),
            ],
            automatic: true,
        }
        .validated()
        .unwrap();
        assert_eq!(settings.domains(), vec!["example.com", "a.example.com"]);
        for name in [
            "https://example.com",
            "example.com:443",
            "*.example.com",
            "a.example.com\nrespond hacked",
            "127.0.0.1",
            "../secrets",
            "localhost",
            "mechamaru.local",
            "MECHAMARU.LOCAL.",
            "app.localhost",
            "app.internal",
            "app.test",
            "app.invalid",
            "app.example",
            "app.home.arpa",
        ] {
            assert!(domain(name).is_err());
        }
    }
    #[test]
    fn manual_mode_loads_keys_and_keeps_https_without_issuers() {
        let managed = Managed {
            client: reqwest::Client::new(),
            socket: "/run/caddy/admin.sock".into(),
            storage: "/data/caddy".into(),
            upstream: "api:3000".into(),
            proxy_token: "0".repeat(64),
        };
        let file = managed.caddyfile(&Settings {
            primary_domain: "video.example.com".into(),
            additional_domains: vec![],
            automatic: false,
        });
        assert!(file.contains("auto_https disable_certs"));
        assert!(file.contains("https://video.example.com"));
        assert!(file.contains("video.example.com.key"));
        assert!(!file.contains("issuer acme"));
        assert!(!file.contains("http://video.example.com"));
        assert!(file.contains("{http.request.remote.host}"));
        assert!(!file.contains("{$GRIMOIRE_PROXY_TOKEN}"));
        assert!(file.contains(&"0".repeat(64)));
    }
}

#[cfg(test)]
mod live_tests {
    use super::*;
    #[tokio::test]
    #[ignore = "Requires GRIMOIRE_TEST_CADDY_BIN pointing to a verified Caddy binary"]
    async fn generated_configs_adapt_with_official_caddy() {
        let binary = std::env::var("GRIMOIRE_TEST_CADDY_BIN")
            .expect("Set GRIMOIRE_TEST_CADDY_BIN to a verified Caddy binary");
        let root =
            std::env::temp_dir().join(format!("grimoire-caddy-adapt-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&root).await.unwrap();
        let managed = Managed {
            client: reqwest::Client::new(),
            socket: root.join("admin.sock"),
            storage: root.join("storage"),
            upstream: "127.0.0.1:3000".into(),
            proxy_token: "0".repeat(64),
        };
        for automatic in [true, false] {
            let file = root.join("Caddyfile");
            tokio::fs::write(
                &file,
                managed.caddyfile(&Settings {
                    primary_domain: "video.example.com".into(),
                    additional_domains: vec!["books.example.com".into()],
                    automatic,
                }),
            )
            .await
            .unwrap();
            let output = tokio::process::Command::new(&binary)
                .args(["adapt", "--adapter", "caddyfile", "--config"])
                .arg(&file)
                .env(
                    "GRIMOIRE_PROXY_TOKEN",
                    "0000000000000000000000000000000000000000000000000000000000000000",
                )
                .output()
                .await
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            let config: Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(
                config["admin"]["listen"],
                format!("unix/{}", managed.socket.display())
            );
            if automatic {
                assert_eq!(
                    config["apps"]["tls"]["automation"]["policies"][0]["issuers"][0]["ca"],
                    LE
                );
            } else {
                assert_eq!(
                    config["apps"]["tls"]["certificates"]["load_files"]
                        .as_array()
                        .unwrap()
                        .len(),
                    2
                );
            }
        }
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}

#[cfg(test)]
mod integration_tests {
    use super::*;
    use crate::auth_integration::{body, register_user, request, state};
    use axum::{
        response::IntoResponse,
        routing::{get, post},
        Router,
    };
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    };
    #[derive(Clone)]
    struct Fake {
        current: Arc<Mutex<Value>>,
        reject: Arc<AtomicBool>,
    }
    #[sqlx::test(migrations = "./migrations")]
    #[ignore = "Requires disposable PostgreSQL DATABASE_URL; run with --ignored"]
    async fn tls_updates_are_admin_only_atomic_revision_guarded_and_preserve_https(
        pool: sqlx::PgPool,
    ) {
        let mut state = state(pool.clone());
        let (owner, csrf, user) = register_user(&state, "tls_owner").await;
        let (other, other_csrf, _) = register_user(&state, "tls_other").await;
        sqlx::query("UPDATE users SET is_admin=true WHERE id=$1")
            .bind(uuid::Uuid::parse_str(user["id"].as_str().unwrap()).unwrap())
            .execute(&pool)
            .await
            .unwrap();
        let root = std::env::temp_dir().join(format!("grimoire-tls-test-{}", uuid::Uuid::new_v4()));
        tokio::fs::create_dir(&root).await.unwrap();
        let socket = root.join("admin.sock");
        let listener = tokio::net::UnixListener::bind(&socket).unwrap();
        let fake = Fake {
            current: Arc::new(Mutex::new(json!({"apps":{},"old":true}))),
            reject: Arc::new(AtomicBool::new(true)),
        };
        let server = Router::new()
            .route(
                "/config/",
                get(|State(fake): State<Fake>| async move {
                    Json(fake.current.lock().unwrap().clone())
                }),
            )
            .route(
                "/adapt",
                post(|_file: String| async move { Json(json!({"result":{"apps":{}}})) }),
            )
            .route(
                "/load",
                post(
                    |State(fake): State<Fake>, Json(config): Json<Value>| async move {
                        if fake.reject.load(Ordering::SeqCst) {
                            return (
                                StatusCode::BAD_REQUEST,
                                Json(json!({"error":"private-proxy-secret /private/key/path"})),
                            )
                                .into_response();
                        }
                        *fake.current.lock().unwrap() = config;
                        StatusCode::OK.into_response()
                    },
                ),
            )
            .with_state(fake.clone());
        let server = tokio::spawn(async move { axum::serve(listener, server).await.unwrap() });
        state.tls = Arc::new(Runtime {
            managed: Some(Managed {
                client: reqwest::Client::builder()
                    .unix_socket(socket.clone())
                    .no_proxy()
                    .build()
                    .unwrap(),
                socket,
                storage: root.clone(),
                upstream: "api:3000".into(),
                proxy_token: "0".repeat(64),
            }),
        });
        let url = "/api/admin/security/tls";
        let origin = "http://localhost:5173";
        let settings =
            json!({"primary_domain":"video.example.com","additional_domains":[],"automatic":true});
        let payload = json!({"expected_revision":0,"settings":settings});
        assert_eq!(
            request(
                &state,
                axum::http::Method::GET,
                url,
                Some(&other),
                None,
                None,
                None
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &state,
                axum::http::Method::PUT,
                url,
                Some(&other),
                Some(&other_csrf),
                Some(origin),
                Some(payload.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        assert_eq!(
            request(
                &state,
                axum::http::Method::PUT,
                url,
                Some(&owner),
                None,
                Some(origin),
                Some(payload.clone())
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let rejected = request(
            &state,
            axum::http::Method::PUT,
            url,
            Some(&owner),
            Some(&csrf),
            Some(origin),
            Some(payload.clone()),
        )
        .await;
        assert_eq!(rejected.status(), StatusCode::BAD_REQUEST);
        let error = body(rejected).await.to_string();
        assert!(error.contains("retained"));
        assert!(!error.contains("private-proxy-secret"));
        assert_eq!(
            sqlx::query_scalar::<_, i64>("SELECT tls_revision FROM instance_security")
                .fetch_one(&pool)
                .await
                .unwrap(),
            0
        );
        assert!(fake.current.lock().unwrap()["old"].as_bool().unwrap());
        fake.reject.store(false, Ordering::SeqCst);
        let response = request(
            &state,
            axum::http::Method::PUT,
            url,
            Some(&owner),
            Some(&csrf),
            Some(origin),
            Some(payload.clone()),
        )
        .await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(body(response).await["revision"], 1);
        assert_eq!(
            request(
                &state,
                axum::http::Method::PUT,
                url,
                Some(&owner),
                Some(&csrf),
                Some(origin),
                Some(payload)
            )
            .await
            .status(),
            StatusCode::CONFLICT
        );
        assert!(allows_origin(&state, "https://video.example.com")
            .await
            .unwrap());
        assert!(
            !allows_origin(&state, "https://video.example.com.evil.test")
                .await
                .unwrap()
        );
        assert!(!allows_origin(&state, "http://video.example.com")
            .await
            .unwrap());
        let policy = json!({"default_action":"allow","allow":[],"deny":[]});
        assert_eq!(
            request(
                &state,
                axum::http::Method::POST,
                "/api/admin/security/ip/preview",
                Some(&owner),
                Some(&csrf),
                Some("https://video.example.com"),
                Some(policy.clone())
            )
            .await
            .status(),
            StatusCode::OK
        );
        assert_eq!(
            request(
                &state,
                axum::http::Method::POST,
                "/api/admin/security/ip/preview",
                Some(&owner),
                Some(&csrf),
                Some("https://video.example.com.evil.test"),
                Some(policy)
            )
            .await
            .status(),
            StatusCode::FORBIDDEN
        );
        let mut off = json!({"expected_revision":1,"settings":settings});
        off["settings"]["automatic"] = json!(false);
        assert_eq!(
            request(
                &state,
                axum::http::Method::PUT,
                url,
                Some(&owner),
                Some(&csrf),
                Some(origin),
                Some(off)
            )
            .await
            .status(),
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            body(
                request(
                    &state,
                    axum::http::Method::GET,
                    url,
                    Some(&owner),
                    None,
                    None,
                    None
                )
                .await
            )
            .await["settings"]["automatic"],
            true
        );
        // Persist actual Caddy renewal events rather than inferring renewals from certificate dates.
        tokio::fs::write(root.join("grimoire.log"),"{\"identifier\":\"video.example.com\",\"ts\":1234,\"level\":\"info\",\"msg\":\"certificate renewed successfully\"}\n").await.unwrap();
        let status = body(
            request(
                &state,
                axum::http::Method::GET,
                url,
                Some(&owner),
                None,
                None,
                None,
            )
            .await,
        )
        .await;
        assert_eq!(status["certificates"][0]["last_successful_renewal"], 1234.0);
        let managed = state.tls.managed.as_ref().unwrap();
        managed.apply_bootstrap(true).await.unwrap();
        // Reconciliation skips an unchanged config, but explicit operator recovery
        // must reprovision it and report proxy failure even when its ID matches.
        fake.reject.store(true, Ordering::SeqCst);
        managed.apply_bootstrap(false).await.unwrap();
        let failure = managed.apply_bootstrap(true).await.unwrap_err();
        assert!(failure.1.contains("retained"));
        assert!(!failure.1.contains("private-proxy-secret"));
        fake.reject.store(false, Ordering::SeqCst);
        managed.apply_bootstrap(true).await.unwrap();
        server.abort();
        tokio::fs::remove_dir_all(root).await.unwrap();
    }
}
