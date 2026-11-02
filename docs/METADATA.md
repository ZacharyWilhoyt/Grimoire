# Metadata, artwork, and organization

Metadata changes **display titles**, never filenames or file contents. Library
bookmarks and recent activity retain their existing source/path keys. Grouping is
an optional catalog view; it does not merge files, identify duplicates, replicate
media, or grant access to anything outside the user's authorized catalog.

## Try it

1. Restart the Rust API to apply migration `0004_metadata.sql`, and restart Vite
   (or rebuild the frontend for deployment).
2. Open a source you own and choose **Gather local metadata**. The background job
   reports progress; repeat scans skip files with unchanged size/mtime. Select
   **Re-read unchanged files** to retry extraction after installing tools or to
   force a refresh. Only one scan runs per source, and one per API instance.
3. Use the information button on a media card or file row. Owners can edit
   display title, creator, album/series, year, media-specific positions, description,
   and genres/subjects (one tag per line). Shared viewers get read-only details.
4. Under **Reviewed online matching**, edit the query, search, preview a candidate,
   and explicitly apply it. Searching and previewing do not change active metadata.
5. Use **Genre** and **Group** controls to filter/group the visible catalog or source.
   Search also includes display title, creator, collection, and tags. Original
   names and paths remain searchable. Grouped music plays in disc/track order;
   TV series open into season folders, and books use series position. Grouping
   normalizes names, separates sources/media types, and includes creator in
   book/music collection identities. This is an initial text-based grouping model, not a canonical work,
   edition, release, or episode database. Distinct collections with identical
   names can still group together; owners can correct their collection labels.

## Folder browsing and group edits

Album/series groups now open as folders by default, with season/disc subdivisions
where relevant. Owners can upload a group cover and edit group tags for all current
and future members within their source. Item credits identify active providers;
the general application notice is separate. See [Media groups](MEDIA_GROUPS.md) for
usage, precedence, identity boundaries and upload limits.

## Optional Arr metadata

The existing local/online workflow remains the default. Owners with Sonarr,
Radarr or Lidarr can additionally configure per-source connections and preview
metadata for existing files. See [Arr integrations](ARR_INTEGRATIONS.md) for setup,
folder mappings, encrypted credentials and the separate approved-network boundary.
No acquisition requests or automatic Arr synchronization are enabled.

## Local extraction

- Audio/video: optional `ffprobe` reads embedded title, artist/album artist, album,
  date, genre, track/disc and selected episode tags. Optional `ffmpeg` creates a
  thumbnail from the first video stream or embedded artwork. Install your OS's
  maintained FFmpeg package (on Ubuntu: `sudo apt install ffmpeg`). Missing tools
  do not prevent browsing/manual edits/online matching; missing tags produce a
  warning and filename fallback. `Show.S02E03` filename hints supply collection,
  season, and episode even before extraction.
- EPUB: package metadata supplies title, creator, subjects, description, year,
  supported series/position tags, and a declared raster cover.
- CBZ: `ComicInfo.xml` supplies title, writer, series/number, year, genres and
  summary. The first raster page in natural filename order supplies the cover.
- Embedded metadata is not automatically extracted while browsing. Gathering is
  owner initiated, so normal catalog requests do not spawn media parsers.

The implementation reads at most 64 MiB of each audio/video file through a pipe;
FFmpeg subprocesses have a 20-second deadline each, bounded output, one decoding
thread for artwork, a 32 MiB per-allocation limit, and no nested file/network
protocols. This favors bounded processing over complete format support: late
metadata, seek-dependent MP4 files, unusual codecs and large attached artwork may
not extract. These subprocess limits are **not an OS sandbox**; maintain FFmpeg
and isolate media processing before accepting arbitrary uploaded media at scale.

Book archives are limited to 128 MiB compressed and 5,000 entries, with bounded
XML reads (1–4 MiB) and cover reads (8 MiB). XML DTDs are rejected and archives are
never extracted onto disk. Raster artwork is decoded with dimension/allocation
limits (4,096 pixels per side, 64 MiB), reduced to at most 600 pixels per side,
and re-encoded as JPEG (maximum 512 KiB). SVG artwork is not accepted. Unsupported
or corrupt books may fail extraction; file browsing and owner editing still work.
The reader's existing independent archive limits remain unchanged.

## Metadata precedence

Metadata layers are stored separately in PostgreSQL:

1. Filename hints + locally extracted fields.
2. Reviewed provider fields (including optional Arr matches) overlay local fields where supplied.
3. An optional complete owner-edited version replaces those display fields.
4. Optional source-scoped album/series covers and tags overlay individual settings.

Local rescans and online matches preserve the complete custom version. **Use
gathered metadata** removes that version and restores automatic artwork selection.
When applying a match, owners can explicitly choose to replace their custom
version. Clearing individual custom fields is allowed; an empty title falls back
to the filename stem. Genres are lowercase, whitespace-normalized and deduplicated,
with at most 20 tags of 60 characters each. They currently share one tag vocabulary;
provider subjects can be broader than conventional genres.

Artwork can use matched-with-local-fallback, local-only, matched-only, or none.
Missing provider artwork produces a warning; applying text metadata still works.
Artwork is cached in PostgreSQL, deduplicated by its encoded image hash, and
served by a private authenticated endpoint. Catalog responses omit descriptions;
the details endpoint supplies them.

## Online providers and setup

Online lookup is enabled by default **only in the sense that an owner can initiate
it**. No automatic provider searches occur on scans, page loads, or playback.
Set `GRIMOIRE_METADATA_ONLINE=false` in `backend/.env` to disable all provider
requests. A review cannot be applied while its provider is disabled.

- **TMDB** movies/television: set `GRIMOIRE_TMDB_TOKEN` to the server operator's
  **API Read Access Token** from the [TMDB API settings](https://developer.themoviedb.org/docs/authentication-application).
  Restart the API after configuration. Never put the token in frontend variables
  or commit it. TV matching uses known season/episode context to fetch the episode
  title/description; otherwise it matches the series. Review the result carefully.
- **MusicBrainz** recordings, with **Cover Art Archive** release covers: no key
  required. The initial adapter uses the first release associated with the chosen
  recording; release/edition selection and automatic track-number matching remain
  future work. Set `GRIMOIRE_METADATA_USER_AGENT` to a meaningful application/version
  and operator/project contact as required by [MusicBrainz](https://musicbrainz.org/doc/MusicBrainz_API/Rate_Limiting).
- **Open Library** works and covers: no key required. Subjects become tags.
  [Search](https://openlibrary.org/dev/docs/api/search) and canonical work records
  can differ; a field absent from a work record stays local/empty rather than being
  guessed. Work-level matching does not select a particular edition/ISBN.

Searching sends the edited query to that provider. Previewing sends its identifier
(and known season/episode numbers for TV). Applying may fetch provider artwork.
The browser never uploads a media file, path, cookie, or API token to a provider;
remote images are fetched by the server and candidates do not load remote
thumbnails. The server's IP and configured User-Agent are visible to providers.
Credentials are sent only to TMDB's API host. Each candidate requires an expiring
30-minute, owner/source/file-bound, single-use review token before application;
clients cannot submit arbitrary metadata/image URLs as provider matches.

Adapters only request fixed HTTPS provider hosts and trusted archive.org cover
redirects, disable ambient proxies, cap JSON/image response bytes, and apply
connection/request deadlines. Redirects are manually checked against the host
allowlist; they cannot lead to arbitrary URLs or internal IP literals. The trusted
provider DNS/redirect infrastructure remains part of the network trust boundary;
operators can additionally restrict worker egress. A PostgreSQL-coordinated budget
spaces **all** provider HTTP requests by 1.1 seconds across API instances, with a
bounded reservation queue. Owner searches/previews are limited to 30 per five
minutes. At most two local/online metadata operations run per API instance.

The app includes per-item **Metadata credits** and general **Application credits**. The general section includes the approved TMDB logo and its
required notice. The bundled logo is unmodified from
[TMDB's official logo page](https://www.themoviedb.org/about/logos-attribution).
Operators remain responsible for applicable provider terms, data/image licenses,
and commercial API arrangements; provider attribution does not grant media rights.

## Permissions and operations

Only the source owner can edit, extract, scan, search, preview or apply metadata.
Source discovery alone and acquisition policies confer no metadata access.
Shared viewers need both visibility and media access for details or artwork.
Authorization is checked on every artwork request, and responses use
`private, no-store` and `nosniff`. Revocation blocks future metadata/artwork
requests as well as delivery. Already rendered metadata/images cannot be recalled
from a recipient's memory or screenshots. Displaying supplied text uses React
text rendering; descriptions are not rendered as arbitrary provider HTML.

Jobs use the existing bounded directory scan (10,000 media files, 100,000 entries,
32 levels). They survive browser navigation but currently run in the API process;
after interruption, a five-minute stale-job lease allows a fresh scan. Unchanged
files already processed are skipped. Jobs are not yet a durable distributed queue;
filesystem stalls and processing isolation remain operational considerations.
Metadata never triggers acquisition requests or creates sharing permissions.

Metadata edits, reset, extraction, and match applications are audited in the
control-plane database. Source disconnection cascades current metadata/reviews/jobs
but preserves audit records. Artwork blobs are bounded individually and deduplicated;
unreferenced blobs, old audit entries, and finished job history currently require
operator retention/cleanup planning. There is no automatic retention deletion or
per-owner storage quota yet. Local directory writers remain trusted, as with
existing media delivery; do not concurrently replace validated files with symlinks.
Uploads need quotas, processing isolation, and lifecycle integration when implemented.

## Verification

```sh
npm --prefix web test
npm --prefix web run build
cargo test --manifest-path backend/Cargo.toml
# Disposable PostgreSQL test role with CREATEDB, never the production role:
DATABASE_URL='postgresql://TEST_ROLE:TEST_PASSWORD@127.0.0.1:5432/TEST_DB' \
  cargo test --manifest-path backend/Cargo.toml -- --ignored
```

Tests cover private artwork and revocation, owner-only operations, preserving
filenames/bookmarks and custom edits, EPUB/CBZ extraction, natural comic cover
selection, XML/URL restrictions, provider parsing, review expiry/file binding/reuse,
and repeated background scans. Live TMDB verification requires operator credentials.

## API reference

All routes require a session. Mutations also require the existing Origin/CSRF
checks. `{source}` and file references always identify an existing registered source.

| Method | Route | Request / purpose |
| --- | --- | --- |
| GET | `/api/metadata/providers` | Enabled providers and configuration notices |
| GET | `/api/sources/{source}/metadata?path=...` | Effective fields, provenance, artwork URL and editability |
| PUT | `/api/sources/{source}/metadata` | `{path, fields, artwork_preference}`; complete custom version |
| DELETE | `/api/sources/{source}/metadata?path=...` | Reset custom version and artwork preference |
| POST | `/api/sources/{source}/metadata/extract` | `{path}`; gather local fields/artwork for one item |
| POST | `/api/sources/{source}/metadata/search` | `{path, provider, query}`; up to six candidates |
| POST | `/api/sources/{source}/metadata/preview` | `{path, provider, id}`; canonical fields and review token |
| POST | `/api/sources/{source}/metadata/apply` | `{path, review_token, replace_custom:false}` |
| POST | `/api/sources/{source}/metadata/scan` | `{force:false}`; start local scan, returns 202 + job ID |
| GET | `/api/sources/{source}/metadata/job` | Latest owner job with counts/status or null |
| GET | `/api/metadata/artwork/{file_key}` | Currently selected JPEG; checks current media access |

Provider identifiers are `tmdb_movie`, `tmdb_tv`, `musicbrainz`, and `openlibrary`.
`artwork_preference` is `auto`, `local`, `matched`, or `none`. The catalog, source
media listing, library, and activity responses add an optional `metadata` view
alongside their original file fields. Existing delivery URLs remain unchanged.

## Stopping a local scan

The source owner can use **Stop scan** while a local metadata job is running.
The API action is `POST /api/sources/{source_id}/metadata/job/{job_id}/cancel`,
requiring the owner's authenticated session, Origin and CSRF token. Media grants
and administrator status on another owner's account do not authorize this action.
The supplied job must belong to that source and owner. Repeating a stop is safe;
completed/failed/interrupted jobs are not changed.

Stopping records the existing `interrupted` state and preserves completed metadata
and source files. Workers check this state between files, including after waiting
for extraction capacity. The current file or bounded directory enumeration may
finish first; stopping does not undo a metadata update already in progress. A
worker retains its extraction/job slot until it actually exits, so starting a new
scan immediately may briefly return the existing busy response. Late worker
completion/failure cannot overwrite the interrupted job state.

If job-status loading or polling fails, **Retry scan status** only reloads status;
it never starts/stops a scan. Displayed progress is labeled last-known until a
successful reload. Scan controls are disabled while status is unknown. Navigating
away cancels pending status reads and polling timers.
