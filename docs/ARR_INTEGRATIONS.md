# Optional per-source Arr metadata connections

The default workflow is unchanged: gather local tags/artwork, edit display titles,
and optionally review TMDB/MusicBrainz/Open Library matches. No Arr installation,
connection, or integration encryption key is required for those features.

Source owners may additionally connect Sonarr, Radarr, and Lidarr. Multiple
connections can belong to one source, including multiple instances of the same
app with different folder mappings. This first version reads metadata for files
already imported into Arr. It does not add media to Arr, search indexers, trigger
downloads, rename files, change monitoring, or modify Arr configuration.

## One-time server setup

1. Update/restart the API to apply migration `0005_arr_connections.sql`.
2. Generate a random 32-byte encryption key, for example:

   ```sh
   openssl rand -hex 32
   ```

   Save the resulting 64 hex characters as `GRIMOIRE_INTEGRATION_KEY` in the
   backend's private environment/configuration. Keep a secure backup alongside
   the database backup, separate from the repository. This is a server encryption
   key, **not** an Arr API key. Never set it as a `VITE_*` frontend variable.
3. Set `GRIMOIRE_ARR_ALLOWED_URLS` to a JSON array of approved service base URLs:

   ```dotenv
   GRIMOIRE_ARR_ALLOWED_URLS='["http://127.0.0.1:8989/","http://127.0.0.1:7878/","http://127.0.0.1:8686/"]'
   ```

   Replace these examples with the addresses reachable **from the Rust API**.
   In Docker, `localhost` is the API's container, not another Arr container. An
   approved URL can include a reverse-proxy prefix, e.g.
   `https://media.example.com/sonarr/`. Approval is for the exact base URL, with
   trailing slash normalization, not an entire hostname or arbitrary endpoint.
   Invalid entries are ignored. Missing/invalid keys or an empty approval list
   disable new connections and requests; defaults continue working.
4. Restart the API after changing these settings. Keep frontend/API served through
   the existing authenticated HTTPS application origin in production.

Arr often runs on private networks, so this is a separate, explicit network trust
boundary from the existing public metadata-provider allowlist. Only the server
operator can approve network destinations. Source owners cannot use this form to
make unrestricted requests from the API host. Approve only trusted services and
DNS names under your control; enforce network egress restrictions where practical.
Approved HTTP URLs transmit the Arr API key without transport encryption: use
HTTP only over a deliberately trusted local/container network, and HTTPS across
untrusted networks. TLS certificate verification remains enabled; self-signed
certificate bypasses and arbitrary proxy settings are not supported.

## Connect a source

Open an **owned source → Optional Arr connections → Add a connection**:

- **Connection name:** a friendly label such as “Home TV”.
- **Arr app:** Sonarr, Radarr, or Lidarr.
- **Base URL:** its operator-approved URL, without `/api/v3` or `/api/v1`.
- **API key:** obtain it from that Arr instance's general settings. The API stores
  it encrypted and never returns the stored value to the browser.
- **Media folder as seen by Arr:** an absolute POSIX or Windows folder path.
- **Folder inside this Grimoire source:** an optional relative folder; blank maps
  to the source root.

Choose **Save connection**, then **Test connection**. Test verifies API discovery,
app identity and authenticated status, and reports the app/API versions. Folder
mapping is validated when previewing an actual file. The adapter discovers the
API version through `GET /api`; it accepts Sonarr API v3/v5, Radarr API v3, and
Lidarr API v1. App release versions and API version numbers are different.
Unsupported future versions fail explicitly instead of guessing request paths.

Example: Grimoire's registered directory is `/srv/media`, while Sonarr sees
`/tv`. Configure Arr media folder `/tv` and Grimoire subfolder `shows`:

```text
Grimoire: /srv/media/shows/Example/Season 01/episode.mkv
Sonarr:   /tv/Example/Season 01/episode.mkv
```

For a dedicated Grimoire TV source whose root already contains `Example/`, leave
its subfolder blank. Mappings are case-sensitive and match full paths by complete
components; files named alike in different directories are not assumed identical.
Path traversal is rejected. Mapped filesystem files must also pass the existing
registered-source/file checks. No new filesystem locations are registered by this
integration, and no Arr-supplied file path is used to read arbitrary local files.

Editing keeps the stored API key when that field is blank. Changing the app or
base URL requires re-entering the key, so a saved secret cannot silently move to a
new destination. Save/disable/remove invalidates pending previews. Removing a
connection leaves already applied metadata/artwork intact and never deletes media.

## Import metadata

Open an item's **ⓘ metadata panel → From your Arr library → Preview from …**.
Review the returned fields, then choose **Apply reviewed match**. Existing custom
edits are preserved unless you check the explicit replacement option. The default
local extraction and online matching controls remain available.

- **Radarr:** finds the movie whose existing movie file matches the mapped path;
  imports title, year, description, genres, collection and cached poster.
- **Sonarr:** finds the series, exact episode file and linked episode(s); imports
  series grouping, season/episode, episode title/description, genres and series
  poster. Multiple linked episode titles are joined; position/description use the
  first episode. Distinct-series path ambiguity fails rather than guessing.
- **Lidarr:** finds the artist, exact track file, linked track and album; imports
  title, artist, album, year, genres, track/disc and cached album cover. Files linked
  to multiple tracks are treated as ambiguous and require manual metadata.

Art is requested only through fixed Arr media-cover API routes, re-encoded and
stored through the existing private artwork cache. Remote image URLs supplied by
Arr are never followed, and API keys are not forwarded to image hosts. Missing
cached artwork produces a warning while allowing text metadata to apply.

No background Arr sync or bulk automatic matching is enabled. Opening a source,
scanning local files, browsing pages, or playing media does not contact Arr. Tests
and previews are explicit owner actions. Changing settings does not alter prior
metadata; future previews use the new configuration.

## Security and operational limits

- Only the source owner can list, save, test, remove or preview its connections.
  Other users and administrators of someone else's source receive no implicit
  access. Acquisition permissions do not authorize these operations.
- Grimoire only sends fixed **GET** requests to Arr. The key itself may confer
  broader Arr privileges; protect it as a privileged credential. Keys use
  XChaCha20-Poly1305 encryption with fresh nonces and authenticated binding to the
  source, connection ID, app and destination. Secret keys/ciphertext are omitted
  from API responses and audit records. The application server necessarily has
  access to decrypted keys during requests.
- HTTP redirects and ambient proxies are disabled. No browser cookies, Grimoire
  session tokens, or credentials from another provider are forwarded. Connection
  responses/errors do not expose Arr's full system status or raw response bodies.
- Connection edits are audited without secrets or service URLs. There are at most
  12 connections per source; 30 tests/previews per owner per five minutes, shared
  metadata concurrency limits, 5-second connection and 15-second HTTP deadlines,
  and 8 MiB per response. Collections exceeding 20,000 records or the response cap
  are rejected; there is no large-library pagination/snapshot cache yet.
- Reviews retain the existing 30-minute expiry, single-use and owner/source/file
  binding, and additionally bind the connection revision. Configuration changes
  during network work invalidate the result before it can be committed. Source
  disconnection cascades its connections and pending previews.
- `GRIMOIRE_METADATA_ONLINE=false` also disables Arr requests. Disabling/removing a
  connection or taking Arr offline does not remove already imported metadata or
  stop local playback. Permission revocation still blocks metadata/artwork delivery.
- Keep `GRIMOIRE_INTEGRATION_KEY` stable and backed up. Changing/losing it makes
  existing encrypted API keys unreadable; re-enter each Arr API key afterward.
  Automatic master-key rotation and remote contributor-node agents are future work.

Before supporting untrusted remote contributors, provide a controlled node/egress
boundary. This implementation deliberately does not let any registered user
choose an arbitrary internal service destination. It also does not implement
acquisition requests; those require their separate default-deny policy workflow.

## API and verification

All endpoints require a session, source ownership and existing mutation
Origin/CSRF checks. `{id}` is a connection UUID.

| Method | Route | Purpose |
| --- | --- | --- |
| GET | `/api/sources/{source}/arr` | Owner settings/status and redacted connections |
| PUT | `/api/sources/{source}/arr/{id}` | Save `{name,kind,base_url,remote_root,source_subpath,api_key,enabled}` |
| DELETE | `/api/sources/{source}/arr/{id}` | Remove a connection and its pending previews |
| POST | `/api/sources/{source}/arr/{id}/test` | Test stored URL/key; return app/API version |
| POST | `/api/sources/{source}/arr/{id}/preview` | `{path}`; return canonical fields and review token |

Use the existing metadata apply endpoint for review tokens. The normal catalog,
metadata and delivery APIs remain unchanged. Tests use isolated simulated Arr
services; no real Arr credentials are included. Integration tests exercise all
three apps, Sonarr API version discovery, encryption/redaction, exact paths,
review reuse/configuration invalidation, private cached artwork, and owner/CSRF
boundaries. Real-instance verification requires your URLs, mappings and API keys
entered through the settings UI.

API contracts: [Sonarr](https://sonarr.tv/docs/api/),
[Radarr](https://radarr.video/docs/api/), [Lidarr](https://lidarr.audio/docs/api/).
