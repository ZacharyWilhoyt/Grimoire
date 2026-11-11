#[path = "source_agent/catalog.rs"]
mod catalog;
#[path = "source_agent/relay.rs"]
mod relay;

use grimoire_api::node_protocol::{self, Enrollment, Heartbeat, Rotation};
use reqwest::{Client, StatusCode, Url};
use ring::{
    rand::SystemRandom,
    signature::{Ed25519KeyPair, KeyPair},
};
use serde::{Deserialize, Serialize};
use std::{fs, io::Write, path::Path, time::Duration};
use uuid::Uuid;

#[derive(Deserialize, Serialize)]
struct Identity {
    node_id: Uuid,
    coordinator: String,
    key: String,
}

fn coordinator(value: &str) -> Result<Url, &'static str> {
    let url = Url::parse(value).map_err(|_| "Use a valid coordinator HTTPS origin.")?;
    if url.scheme() != "https"
        || url.host().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("Use a coordinator HTTPS origin without credentials or a path.");
    }
    Ok(url)
}

fn read_identity(path: &Path) -> Result<Identity, &'static str> {
    let metadata =
        fs::symlink_metadata(path).map_err(|_| "The node identity file is unavailable.")?;
    if !metadata.is_file() || metadata.len() > 4096 {
        return Err("The node identity must be a small regular file.");
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if metadata.permissions().mode() & 0o077 != 0 {
            return Err("Set the node identity file permissions to 0600.");
        }
    }
    serde_json::from_slice(&fs::read(path).map_err(|_| "Could not read the node identity.")?)
        .map_err(|_| "Could not parse the node identity.")
}

fn create_identity(
    path: &Path,
    coordinator: &Url,
    node_id: Uuid,
) -> Result<Identity, &'static str> {
    let key = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new())
        .map_err(|_| "Could not generate a node key.")?;
    let identity = Identity {
        node_id,
        coordinator: coordinator.to_string(),
        key: hex::encode(key.as_ref()),
    };
    let bytes = serde_json::to_vec(&identity).map_err(|_| "Could not encode the node identity.")?;
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|_| "Could not create the node identity file.")?;
    file.write_all(&bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| "Could not persist the node identity.")?;
    Ok(identity)
}

fn key(identity: &Identity) -> Result<Ed25519KeyPair, &'static str> {
    let bytes = hex::decode(&identity.key).map_err(|_| "Invalid node identity key.")?;
    Ed25519KeyPair::from_pkcs8(&bytes).map_err(|_| "Invalid node identity key.")
}

async fn send_heartbeat(
    client: &Client,
    url: &Url,
    identity: &Identity,
    key: &Ed25519KeyPair,
) -> Result<StatusCode, ()> {
    let mut heartbeat = Heartbeat {
        version: node_protocol::VERSION,
        node_id: identity.node_id,
        timestamp_ms: node_protocol::now_ms(),
        signature: String::new(),
    };
    heartbeat.signature = hex::encode(key.sign(&heartbeat.signed_bytes()).as_ref());
    client
        .post(url.join(node_protocol::HEARTBEAT).map_err(|_| ())?)
        .json(&heartbeat)
        .send()
        .await
        .map(|response| response.status())
        .map_err(|_| ())
}

async fn enroll(
    client: &Client,
    path: &Path,
    origin: &str,
    label: &str,
) -> Result<(), &'static str> {
    let url = coordinator(origin)?;
    let mut pending_path = path.as_os_str().to_owned();
    pending_path.push(".enroll");
    let pending_path = Path::new(&pending_path);
    let (identity, pending) = match fs::symlink_metadata(path) {
        Ok(_) => {
            let current = read_identity(path)?;
            if current.coordinator != url.as_str() {
                return Err("The node identity is already bound to another coordinator.");
            }
            match send_heartbeat(client, &url, &current, &key(&current)?).await {
                Ok(StatusCode::NO_CONTENT) => {
                    println!("Source Agent {} is already enrolled.", current.node_id);
                    return Ok(());
                }
                Ok(StatusCode::UNAUTHORIZED) => {}
                _ => {
                    return Err(
                        "Could not verify the existing agent. Check the coordinator and retry.",
                    )
                }
            }
            let replacement = match fs::symlink_metadata(pending_path) {
                Ok(_) => read_identity(pending_path)?,
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                    create_identity(pending_path, &url, current.node_id)?
                }
                Err(_) => return Err("Could not inspect the pending enrollment key."),
            };
            if replacement.node_id != current.node_id
                || replacement.coordinator != current.coordinator
            {
                return Err("The pending enrollment belongs to a different node or coordinator.");
            }
            // Recover if the coordinator accepted the key before a previous run lost its response.
            if send_heartbeat(client, &url, &replacement, &key(&replacement)?).await
                == Ok(StatusCode::NO_CONTENT)
            {
                fs::rename(pending_path, path).map_err(|_| {
                    "Could not install the accepted key. Retry enrollment to recover."
                })?;
                println!("Enrolled Source Agent {}.", replacement.node_id);
                return Ok(());
            }
            (replacement, true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            (create_identity(path, &url, Uuid::new_v4())?, false)
        }
        Err(_) => return Err("Could not inspect the node identity file."),
    };
    let key = key(&identity)?;
    let token = rpassword::prompt_password("Enrollment token: ")
        .map_err(|_| "Could not read the enrollment token.")?;
    if node_protocol::decode_hex::<32>(token.trim()).is_none() {
        return Err("The enrollment token is invalid.");
    }
    let mut message = Enrollment {
        version: node_protocol::VERSION,
        node_id: identity.node_id,
        label: label.into(),
        public_key: hex::encode(key.public_key().as_ref()),
        token: token.trim().into(),
        signature: String::new(),
    };
    message.signature = hex::encode(key.sign(&message.signed_bytes()).as_ref());
    let response = client
        .post(
            url.join(node_protocol::ENROLL)
                .map_err(|_| "Invalid enrollment endpoint.")?,
        )
        .json(&message)
        .send()
        .await
        .map_err(|_| "Could not contact the coordinator.")?;
    if response.status() != StatusCode::CREATED {
        return Err("Enrollment was rejected. Check the token, label and coordinator settings.");
    }
    if pending {
        fs::rename(pending_path, path)
            .map_err(|_| "Could not install the accepted key. Retry enrollment to recover.")?;
    }
    println!("Enrolled Source Agent {}.", identity.node_id);
    Ok(())
}

async fn rotate(client: &Client, path: &Path) -> Result<(), &'static str> {
    let identity = read_identity(path)?;
    let url = coordinator(&identity.coordinator)?;
    let old_key = key(&identity)?;
    let mut pending_path = path.as_os_str().to_owned();
    pending_path.push(".rotate");
    let pending_path = Path::new(&pending_path);
    let pending = match fs::symlink_metadata(pending_path) {
        Ok(_) => read_identity(pending_path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            create_identity(pending_path, &url, identity.node_id)?
        }
        Err(_) => return Err("Could not inspect the pending node identity."),
    };
    if pending.node_id != identity.node_id || pending.coordinator != identity.coordinator {
        return Err("The pending key belongs to a different node or coordinator.");
    }
    let new_key = key(&pending)?;
    if send_heartbeat(client, &url, &pending, &new_key).await != Ok(StatusCode::NO_CONTENT) {
        let mut message = Rotation {
            version: node_protocol::VERSION,
            node_id: identity.node_id,
            timestamp_ms: node_protocol::now_ms(),
            new_public_key: hex::encode(new_key.public_key().as_ref()),
            signature: String::new(),
            new_signature: String::new(),
        };
        let bytes = message.signed_bytes();
        message.signature = hex::encode(old_key.sign(&bytes).as_ref());
        message.new_signature = hex::encode(new_key.sign(&bytes).as_ref());
        let response = client
            .post(
                url.join(node_protocol::ROTATE)
                    .map_err(|_| "Invalid rotation endpoint.")?,
            )
            .json(&message)
            .send()
            .await
            .map_err(|_| "Could not contact the coordinator. Retry rotation to recover.")?;
        if response.status() != StatusCode::NO_CONTENT {
            return Err("Key rotation was rejected. Keep the pending file and retry.");
        }
    }
    fs::rename(pending_path, path)
        .map_err(|_| "Could not install the accepted key. Retry rotation to recover.")?;
    println!("Rotated key for Source Agent {}.", identity.node_id);
    Ok(())
}

async fn run(client: &Client, path: &Path) -> Result<(), &'static str> {
    let identity = read_identity(path)?;
    let url = coordinator(&identity.coordinator)?;
    let key = key(&identity)?;
    let config = std::env::var_os("GRIMOIRE_AGENT_CONFIG");
    let configuration = config.as_deref().map(Path::new);
    let maintenance = maintain(client, &url, &identity, &key, path, configuration);
    let delivery = async {
        if let Some(config) = configuration {
            relay::run(client, &url, &identity, &key, path, config).await;
        }
        std::future::pending::<()>().await
    };
    tokio::select! {
        _ = tokio::signal::ctrl_c() => Ok(()),
        result = maintenance => result,
        _ = delivery => Ok(()),
    }
}

async fn maintain(
    client: &Client,
    url: &Url,
    identity: &Identity,
    key: &Ed25519KeyPair,
    path: &Path,
    config: Option<&Path>,
) -> Result<(), &'static str> {
    let mut retry_seconds = 2;
    loop {
        let status = send_heartbeat(client, url, identity, key).await;
        if status == Ok(StatusCode::NO_CONTENT) {
            if let Some(config) = config {
                if let Err(message) = catalog::sync(client, url, identity, key, path, config).await
                {
                    eprintln!("{message}");
                }
            }
        }
        let delay = match status {
            Ok(StatusCode::NO_CONTENT) => {
                retry_seconds = 2;
                30
            }
            Ok(StatusCode::UPGRADE_REQUIRED) => {
                return Err("The coordinator requires a different node protocol version.")
            }
            _ => {
                eprintln!("Heartbeat unavailable. Retrying in {retry_seconds}s.");
                let delay = retry_seconds;
                retry_seconds = (retry_seconds * 2).min(60);
                delay
            }
        };
        tokio::time::sleep(Duration::from_secs(delay)).await;
    }
}

async fn execute() -> Result<(), &'static str> {
    let path =
        std::env::var_os("GRIMOIRE_AGENT_STATE").unwrap_or_else(|| "grimoire-agent.json".into());
    let path = Path::new(&path);
    let mut client = Client::builder()
        .https_only(true)
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(10));
    if let Some(ca) = std::env::var_os("GRIMOIRE_AGENT_CA_FILE") {
        let path = Path::new(&ca);
        let metadata =
            fs::metadata(path).map_err(|_| "Could not inspect the coordinator CA file.")?;
        if !metadata.is_file() || metadata.len() > 128 * 1024 {
            return Err("The coordinator CA file is invalid.");
        }
        let bytes = fs::read(path).map_err(|_| "Could not read the coordinator CA file.")?;
        let certificate = reqwest::Certificate::from_pem(&bytes)
            .map_err(|_| "Invalid coordinator CA certificate.")?;
        client = client.add_root_certificate(certificate);
    }
    let client = client
        .build()
        .map_err(|_| "Could not initialize the HTTPS client.")?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [command, origin, label] if command == "enroll" => {
            enroll(&client, path, origin, label).await
        }
        [command] if command == "run" => run(&client, path).await,
        [command] if command == "rotate" => rotate(&client, path).await,
        _ => {
            Err("Usage: grimoire-source-agent enroll COORDINATOR_HTTPS_ORIGIN LABEL | run | rotate")
        }
    }
}

#[tokio::main]
async fn main() {
    if let Err(error) = execute().await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
