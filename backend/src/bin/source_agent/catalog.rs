use super::Identity;
use grimoire_api::node_protocol::{self, CatalogDelta, CatalogEntry, SourceGrant};
use reqwest::{Client, StatusCode, Url};
use ring::signature::Ed25519KeyPair;
use serde::{de::DeserializeOwned, Deserialize, Serialize};
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    time::UNIX_EPOCH,
};
use uuid::Uuid;

const MAX_SCAN_ENTRIES: usize = 100_000;
const MAX_FILES: usize = 10_000;
const MAX_SOURCES: usize = 32;
const STATE_LIMIT: u64 = 64 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    sources: Vec<Source>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Source {
    id: Uuid,
    name: String,
    root: PathBuf,
    #[serde(default)]
    published: bool,
    #[serde(default)]
    grants: Vec<SourceGrant>,
}

#[derive(Clone, Serialize, Deserialize)]
struct Indexed {
    path: String,
    fingerprint: String,
    entry: CatalogEntry,
}

#[derive(Serialize, Deserialize)]
struct Pending {
    delta: CatalogDelta,
    added: Vec<Indexed>,
}

#[derive(Default, Serialize, Deserialize)]
struct Index {
    name: String,
    revision: i64,
    published: bool,
    grants: Vec<SourceGrant>,
    files: BTreeMap<String, Indexed>,
    pending: Option<Pending>,
}

fn read<T: DeserializeOwned>(path: &Path, limit: u64) -> Result<T, &'static str> {
    let metadata = fs::symlink_metadata(path).map_err(|_| "Catalog file unavailable.")?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err("Invalid catalog configuration or index file.");
    }
    let mut bytes = Vec::new();
    fs::File::open(path)
        .map_err(|_| "Catalog file unavailable.")?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "Catalog file unavailable.")?;
    if bytes.len() as u64 > limit {
        return Err("Catalog file exceeds its size limit.");
    }
    serde_json::from_slice(&bytes).map_err(|_| "Invalid catalog configuration or index JSON.")
}

fn persist(path: &Path, index: &Index) -> Result<(), &'static str> {
    let temporary = path.with_extension(format!("{}.tmp", Uuid::new_v4()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let result = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Could not create index journal.")?;
        serde_json::to_writer(&mut file, index)
            .map_err(|_| "Could not serialize index journal.")?;
        file.flush()
            .and_then(|_| file.sync_all())
            .map_err(|_| "Could not persist index journal.")?;
        fs::rename(&temporary, path).map_err(|_| "Could not commit index journal.")
    })();
    if result.is_err() {
        let _ = fs::remove_file(temporary);
    }
    result
}

fn fingerprint(metadata: &fs::Metadata, path: &str) -> Result<String, &'static str> {
    let modified = metadata
        .modified()
        .map_err(|_| "Could not inspect media timestamps.")?
        .duration_since(UNIX_EPOCH)
        .map_err(|_| "Invalid media timestamp.")?
        .as_nanos();
    #[cfg(unix)]
    let location = {
        use std::os::unix::fs::MetadataExt;
        let _ = path;
        format!("{}:{}", metadata.dev(), metadata.ino())
    };
    #[cfg(not(unix))]
    let location = path.to_owned();
    Ok(format!("{location}:{}:{modified}", metadata.len()))
}

fn scan(
    root: &Path,
    previous: &BTreeMap<String, Indexed>,
) -> Result<BTreeMap<String, Indexed>, &'static str> {
    if !root.is_absolute() || fs::canonicalize(root).ok().as_deref() != Some(root) || !root.is_dir()
    {
        return Err("Approved media roots must be absolute canonical directories.");
    }
    let mut pending = vec![(root.to_path_buf(), 0)];
    let mut candidates = BTreeMap::new();
    let mut visited = 0;
    while let Some((directory, depth)) = pending.pop() {
        if fs::canonicalize(&directory).ok().as_ref() != Some(&directory) {
            return Err("A source directory moved during the scan.");
        }
        for item in fs::read_dir(directory).map_err(|_| "Could not complete the source scan.")? {
            visited += 1;
            if visited > MAX_SCAN_ENTRIES {
                return Err("Source scan exceeded its directory entry limit.");
            }
            let item = item.map_err(|_| "Could not complete the source scan.")?;
            let kind = item
                .file_type()
                .map_err(|_| "Could not inspect a source entry.")?;
            let path = item.path();
            if kind.is_dir() {
                if depth >= 32 {
                    return Err("Source scan exceeded its directory depth limit.");
                }
                pending.push((path, depth + 1));
            } else if kind.is_file() {
                let Some(extension) = path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .map(str::to_ascii_lowercase)
                else {
                    continue;
                };
                if node_protocol::media_kind(&extension).is_none() {
                    continue;
                }
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if name.trim() != name
                    || name.chars().count() > 255
                    || name.contains('\\')
                    || name.chars().any(char::is_control)
                {
                    continue;
                }
                let Some(relative) = path.strip_prefix(root).ok().and_then(Path::to_str) else {
                    continue;
                };
                if relative.len() > 4096 {
                    continue;
                }
                let metadata = item
                    .metadata()
                    .map_err(|_| "Could not inspect a media file.")?;
                if !metadata.is_file() || metadata.len() == 0 || metadata.len() > i64::MAX as u64 {
                    continue;
                }
                candidates.insert(
                    relative.to_owned(),
                    (
                        name.to_owned(),
                        extension,
                        metadata.len() as i64,
                        fingerprint(&metadata, relative)?,
                    ),
                );
                if candidates.len() > MAX_FILES {
                    return Err("Source scan exceeded its media item limit.");
                }
            }
        }
    }

    let mut old_fingerprints = HashMap::<&str, Vec<&Indexed>>::new();
    for file in previous.values() {
        old_fingerprints
            .entry(&file.fingerprint)
            .or_default()
            .push(file);
    }
    let mut new_fingerprints = HashMap::<String, usize>::new();
    for (_, _, _, fingerprint) in candidates.values() {
        *new_fingerprints.entry(fingerprint.clone()).or_default() += 1;
    }
    Ok(candidates
        .into_iter()
        .map(|(path, (name, extension, size_bytes, fingerprint))| {
            let old = previous
                .get(&path)
                .filter(|item| item.fingerprint == fingerprint)
                .or_else(|| {
                    let matches = old_fingerprints.get(fingerprint.as_str())?;
                    (matches.len() == 1 && new_fingerprints[&fingerprint] == 1)
                        .then_some(matches[0])
                });
            let (item_id, representation_id) = old.map_or_else(
                || (Uuid::new_v4(), Uuid::new_v4()),
                |item| (item.entry.item_id, item.entry.representation_id),
            );
            (
                path.clone(),
                Indexed {
                    path,
                    fingerprint,
                    entry: CatalogEntry {
                        item_id,
                        representation_id,
                        name,
                        extension,
                        size_bytes,
                    },
                },
            )
        })
        .collect())
}

async fn send_pending(
    client: &Client,
    url: &Url,
    identity: &Identity,
    key: &Ed25519KeyPair,
    path: &Path,
    index: &mut Index,
) -> Result<(), &'static str> {
    let Some(pending) = index.pending.as_ref() else {
        return Ok(());
    };
    let body = serde_json::to_vec(&pending.delta).map_err(|_| "Could not encode catalog delta.")?;
    let timestamp = node_protocol::now_ms();
    let message = node_protocol::catalog_signature_bytes(
        node_protocol::VERSION,
        identity.node_id,
        timestamp,
        &node_protocol::body_digest(&body),
    );
    let response = client
        .post(
            url.join(node_protocol::CATALOG_DELTA)
                .map_err(|_| "Invalid catalog endpoint.")?,
        )
        .header(
            "x-grimoire-node-version",
            node_protocol::VERSION.to_string(),
        )
        .header("x-grimoire-node-id", identity.node_id.to_string())
        .header("x-grimoire-node-timestamp", timestamp.to_string())
        .header(
            "x-grimoire-node-signature",
            hex::encode(key.sign(&message).as_ref()),
        )
        .header("content-type", "application/json")
        .body(body)
        .send()
        .await
        .map_err(|_| "Could not send catalog delta.")?;
    if response.status() != StatusCode::NO_CONTENT {
        return Err("Coordinator rejected the catalog delta. The previous catalog remains intact.");
    }
    let pending = index.pending.take().expect("pending catalog delta exists");
    index.revision = pending.delta.revision;
    index.name = pending.delta.source_name;
    index.published = pending.delta.published;
    if let Some(grants) = pending.delta.grants {
        index.grants = grants;
    }
    let changed: HashSet<_> = pending
        .delta
        .removed
        .into_iter()
        .chain(pending.added.iter().map(|item| item.entry.item_id))
        .collect();
    index
        .files
        .retain(|_, item| !changed.contains(&item.entry.item_id));
    index.files.extend(
        pending
            .added
            .into_iter()
            .map(|item| (item.path.clone(), item)),
    );
    persist(path, index)
}

async fn sync_source(
    client: &Client,
    url: &Url,
    identity: &Identity,
    key: &Ed25519KeyPair,
    state: &Path,
    source: &Source,
) -> Result<(), &'static str> {
    let path = state.join(format!("{}.json", source.id));
    let mut index: Index = if path.exists() {
        read(&path, STATE_LIMIT)?
    } else {
        Index::default()
    };
    send_pending(client, url, identity, key, &path, &mut index).await?;
    let new = if source.published {
        let source_root = source.root.clone();
        let previous = std::mem::take(&mut index.files);
        let (new, previous) = tokio::task::spawn_blocking(move || {
            let next = scan(&source_root, &previous);
            (next, previous)
        })
        .await
        .map_err(|_| "Source scan worker stopped.")?;
        index.files = previous;
        new?
    } else {
        index.files.clone()
    };
    let old_by_id: HashMap<_, _> = index
        .files
        .values()
        .map(|item| (item.entry.item_id, item))
        .collect();
    let new_ids: HashSet<_> = new.values().map(|item| item.entry.item_id).collect();
    let mut removed: Vec<_> = index
        .files
        .values()
        .filter(|item| !new_ids.contains(&item.entry.item_id))
        .map(|item| item.entry.item_id)
        .collect();
    let mut added: Vec<_> = new
        .into_values()
        .filter(|item| {
            old_by_id
                .get(&item.entry.item_id)
                .is_none_or(|old| old.entry != item.entry || old.path != item.path)
        })
        .collect();
    let grants_changed = index.revision == 0 || index.grants != source.grants;
    if index.revision > 0
        && !index.published
        && !source.published
        && index.name == source.name
        && !grants_changed
    {
        return Ok(());
    }
    loop {
        let limit = node_protocol::CATALOG_BATCH_LIMIT;
        let batch_removed = removed.split_off(removed.len().saturating_sub(limit));
        let batch_added = added.split_off(added.len().saturating_sub(limit - batch_removed.len()));
        let final_batch = removed.is_empty() && added.is_empty();
        index.pending = Some(Pending {
            delta: CatalogDelta {
                source_id: source.id,
                source_name: source.name.clone(),
                revision: index
                    .revision
                    .checked_add(1)
                    .ok_or("Catalog revision limit exceeded.")?,
                published: final_batch && source.published,
                upserts: batch_added.iter().map(|item| item.entry.clone()).collect(),
                removed: batch_removed,
                grants: (final_batch && grants_changed).then(|| source.grants.clone()),
            },
            added: batch_added,
        });
        persist(&path, &index)?;
        send_pending(client, url, identity, key, &path, &mut index).await?;
        if final_batch {
            return Ok(());
        }
    }
}

pub(super) async fn sync(
    client: &Client,
    url: &Url,
    identity: &Identity,
    key: &Ed25519KeyPair,
    state_path: &Path,
    config_path: &Path,
) -> Result<(), &'static str> {
    let config: Configuration = read(config_path, 128 * 1024)?;
    let mut ids = HashSet::new();
    if config.sources.len() > MAX_SOURCES
        || config
            .sources
            .iter()
            .any(|source| source.id.is_nil() || !ids.insert(source.id))
    {
        return Err("Set a unique UUID for each approved media source, with at most 32 sources.");
    }
    let mut folder = state_path.as_os_str().to_owned();
    folder.push(".catalog");
    let folder = Path::new(&folder);
    if !folder.exists() {
        let mut options = fs::DirBuilder::new();
        #[cfg(unix)]
        {
            use std::os::unix::fs::DirBuilderExt;
            options.mode(0o700);
        }
        options
            .create(folder)
            .map_err(|_| "Could not create the private catalog state directory.")?;
    }
    if !fs::symlink_metadata(folder).is_ok_and(|metadata| metadata.is_dir()) {
        return Err("The catalog state directory is invalid.");
    }
    for source in config.sources {
        if let Err(message) = sync_source(client, url, identity, key, folder, &source).await {
            eprintln!("Catalog update failed for source {}. {message}", source.id);
        }
    }
    Ok(())
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[test]
    fn scan_keeps_ids_across_a_rename_and_excludes_symlinks() {
        let root = std::env::temp_dir().join(format!("grimoire-agent-scan-{}", Uuid::new_v4()));
        fs::create_dir(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let media = root.join("before.mkv");
        fs::write(&media, b"fixture").unwrap();
        std::os::unix::fs::symlink(&media, root.join("link.mkv")).unwrap();
        let before = scan(&root, &BTreeMap::new()).unwrap();
        assert_eq!(before.len(), 1);
        fs::create_dir(root.join("Season 1")).unwrap();
        fs::rename(media, root.join("Season 1/after.mkv")).unwrap();
        let after = scan(&root, &before).unwrap();
        assert_eq!(after.len(), 1);
        assert_eq!(
            before["before.mkv"].entry.item_id,
            after["Season 1/after.mkv"].entry.item_id
        );
        fs::remove_dir_all(root).unwrap();
    }
}
