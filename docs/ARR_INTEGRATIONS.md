# Optional per-source Arr metadata connections

The default workflow is unchanged: gather local tags/artwork, edit display titles,
and optionally review TMDB/MusicBrainz/Open Library matches. No Arr installation,
connection, or integration encryption key is required for those features.

Source owners may additionally connect Sonarr, Radarr, and Lidarr. Multiple
connections can belong to one source, including multiple instances of the same
app with different folder mappings. The metadata connection reads files already imported into Arr. Managed container
setup separately creates the Arr instance and configures its login.

## Create containers from Grimoire

The optional managed controller provisions Sonarr, Radarr, and Lidarr for a
source-owning administrator. Each instance has its own app configuration,
credentials, and source-folder mapping. The 12-connection limit applies to the
combined list of managed and manually connected instances.

On the Docker host, start the base deployment, then run this once from the repo:

```sh
python3 scripts/setup-managed-arr.py --media-root /media
```

Repeat `--media-root` for each existing read-only API mount destination you want
to approve. With custom mounts, use the actual paths **inside the API container**.
The helper uses the current `deploy/.env`, including its database selection. It
pins the local LinuxServer stable images by digest, creates approved media
volumes, starts the private controller, and enables the API connection to it.
Use `--max-instances` to adjust the host-wide limit (12 by default). Repeat the
helper after changing approved mounts. It requires a local Docker Engine with
API v1.47 or later and Compose v2. The running controller and its configuration
are retained across ordinary API rebuilds.

1. Open an owned source, expand **Optional Arr connections**, and choose
   **Create managed Arr instance**.
2. Choose the app, name, source folder, and a separate Arr login. The selected
   folder appears as `/media` inside that container. Media defaults to read-only.
   Enable write access per instance if you want Arr to import, rename, or manage
   files there. Writable instances run as UID 10001 and receive the selected
   folder’s non-root group. The host or network share must grant that user or
   group write access. Grimoire leaves filesystem ownership and permissions
   to the operator.
3. Watch the card until it reports **running**. The connection and its encrypted
   API key are already saved. **Open local Arr UI** on the server (or forward its
   loopback port through SSH) to configure the Arr library. Add `/media` as the
   root folder and import the existing library using Arr's own interface. Some
   Arr imports require writable media. Arr instances start with empty libraries.
4. Use **Test connection** and then preview matches from the item's metadata
   panel. Repeat creation for additional apps or another instance of the same app.

Start, stop, restart and remove a managed container from its card. Container
removal retains the Arr configuration and applied metadata. After it finishes,
remove the connection from the list. Remove managed instances before disconnecting
their source. Use a replacement instance to change its app, folder or mount access.

The controller has Docker daemon privileges. It receives a fixed set of requests
over a private Unix socket and accepts only approved image templates, media
volumes and owned instance identifiers. The Grimoire API mounts that socket, while
the Docker socket stays with the controller. Arr applications run as UID 10001
with dropped capabilities, a read-only root filesystem, private config and bounded
CPU/memory/processes. The selected media subpath stays within an operator-approved
volume. The Arr UIs bind to loopback after authenticated setup. The main Caddy
site continues to serve Grimoire, and the metadata adapter uses its existing fixed
GET requests. Protect the Docker host and its private `arr_state` volume, which
contains Arr credentials and library settings. Back up that volume together with
the database and integration encryption key. Configuration from removed instances
is retained there, so its disk usage needs operator monitoring.

Tested container images are maintained by [LinuxServer.io](https://docs.linuxserver.io/).
Use the existing connection workflow below for separately operated instances.

## One-time setup for existing instances

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

## Preview a whole Sonarr series

Open the grouped video series, choose **Edit series metadata**, and select
**Preview series from Arr library**. Grimoire matches existing episode files by
mapped path within that grouping. Review the synopsis and the matched files,
then apply either update or both. Reviews support up to 1,000 matched files.

The synopsis is stored on the group and displayed on its series page. You can
also enter a synopsis in the group editor. Bulk episode imports preserve manual
episode edits and the current group identity. Files in other groups and files
without exact matches stay outside the review.

## Display exported artwork and browse seasons

Grimoire recognizes the artwork files Sonarr exports alongside existing video
files. Use `banner.jpg` (or `fanart.jpg`) and `poster.jpg` in the series folder,
`season02-poster.jpg` for Season 2, and `<episode filename stem>-thumb.jpg` beside
an episode. JPEG, PNG, and WebP are supported. Season 0 also recognizes
`season-specials-poster.jpg`.

A grouped video series lists its seasons when season numbers are available.
Each season has a shareable URL, its own poster when present, and a list of
matching episodes. Missing season art falls back to the series poster. Episode
cards prefer their matching thumbnail. The link and artwork routes require
current source access. Grimoire reads these files directly, so exported artwork
appears without an additional Arr metadata import.

## Security and operational limits

- Only the source owner can list, save, test, remove or preview its connections.
  Other users and administrators of someone else's source receive no implicit
  access. Acquisition permissions do not authorize these operations.
- The metadata adapter sends fixed **GET** requests to Arr. The key itself may confer
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
| POST | `/api/sources/{source}/arr/managed` | Reserve and provision a managed instance (owning administrator) |
| POST | `/api/sources/{source}/arr/{id}/manage/{action}` | Start, stop, restart or remove a managed container (owning administrator) |
| PUT | `/api/sources/{source}/arr/{id}` | Save `{name,kind,base_url,remote_root,source_subpath,api_key,enabled}` |
| DELETE | `/api/sources/{source}/arr/{id}` | Remove a connection and its pending previews |
| POST | `/api/sources/{source}/arr/{id}/test` | Test stored URL/key; return app/API version |
| POST | `/api/sources/{source}/arr/{id}/preview` | `{path}`; return canonical fields and review token |
| POST | `/api/sources/{source}/groups/{group}/arr/{id}/preview` | Review matched Sonarr episodes and a group synopsis |
| POST | `/api/sources/{source}/groups/{group}/arr/apply` | Apply a reviewed series synopsis, matched episodes, or both |
| PUT | `/api/sources/{source}/groups/{group}/synopsis` | Save a manual group synopsis |

Use the file metadata apply endpoint for individual reviews and the group apply endpoint for series reviews. The normal catalog,
metadata and delivery APIs remain unchanged. Tests use isolated simulated Arr
services; no real Arr credentials are included. Integration tests exercise all
three apps, Sonarr API version discovery, encryption/redaction, exact paths,
review reuse/configuration invalidation, private cached artwork, and owner/CSRF
boundaries. Real-instance verification requires your URLs, mappings and API keys
entered through the settings UI.

API contracts: [Sonarr](https://sonarr.tv/docs/api/),
[Radarr](https://radarr.video/docs/api/), [Lidarr](https://lidarr.audio/docs/api/).
