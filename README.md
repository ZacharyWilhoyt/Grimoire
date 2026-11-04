# Grimoire

**A decentralized, community-powered multimedia platform.**

Grimoire is an ambitious multimedia platform designed to bring video
streaming, music playback, and digital book reading together in one
unified experience. It combines centralized identity management with
distributed, user-contributed media storage and delivery infrastructure.

Rather than depending entirely on a single operator's servers to store
and deliver every file, Grimoire is designed to let participating
content sources contribute storage and bandwidth to the network---while
retaining control over their content, infrastructure, and sharing
permissions.

Grimoire aims to bring together the features people appreciate across
existing media platforms with a scalable, community-powered
architecture.

> **Project status:** Grimoire has a working local prototype with accounts,
> source permissions, media playback/readers, metadata and saved progress.
> Distributed contributor delivery, user uploads, acquisition requests and public
> deployment remain planned. See [Development Status](#development-status) for
> implemented capabilities and the remaining decisions.

## Table of Contents

-   [Vision](#vision)
-   [Core Features and Goals](#core-features-and-goals)
-   [Unified Multimedia Experience](#unified-multimedia-experience)
-   [Distributed Content Sources and
    CDN](#distributed-content-sources-and-cdn)
-   [Media Acquisition](#media-acquisition)
-   [Content Ownership and
    Permissions](#content-ownership-and-permissions)
-   [Identity and Architecture](#identity-and-architecture)
-   [Security Principles](#security-principles)
-   [Scalability and Resilience](#scalability-and-resilience)
-   [Project Scope](#project-scope)
-   [Planned Architecture
    Considerations](#planned-architecture-considerations)
-   [Development Status](#development-status)
-   [Local Development](#local-development)
-   [Checks and Operations](#checks-and-operations)
-   [Contributing](#contributing)
-   [Responsible Use](#responsible-use)
-   [License](#license)

## Vision

Grimoire's goal is to build a secure, decentralized, community-powered
multimedia platform that unifies video streaming, music playback, and
digital book reading. It will leverage distributed, user-contributed
storage and CDN infrastructure to achieve scalable content delivery
while maintaining centralized identity management, granular content
ownership, and enforceable authorization policies.

The platform is intended to serve as an alternative to existing media
consumption services. Video, music, and books are all first-class parts
of the project; Grimoire is not intended to be a video-only service with
secondary media features bolted on.

The underlying principle is:

**Contributing storage or bandwidth to Grimoire should not require
giving up ownership or control over who can access a contributor's
media.**

## Core Features and Goals

Grimoire's intended capabilities include:

-   A unified web application for video, music, and books.
-   A searchable media catalog and discovery experience.
-   Community-contributed media storage and delivery.
-   Distributed CDN/source selection for media delivery.
-   Configurable media acquisition through integrations with the \*Arr
    ecosystem.
-   Content-source visibility and sharing controls.
-   Per-user, per-source acquisition permissions.
-   Centralized authentication and account management in the initial
    design.
-   Granular authorization for media access and acquisition.
-   A modular architecture that can evolve as the platform grows.
-   Security, scalability, and resilience as foundational design goals.

These are project goals. Their implementation status may vary as
development progresses.

## Unified Multimedia Experience

Grimoire brings multiple media types into one application.

### Video

The platform is intended to support video content such as anime,
television, movies, and other supported media. Planned playback
considerations include:

-   Streaming and playback controls
-   Quality selection
-   Audio and subtitle track selection
-   Playback progress and history
-   Media metadata and catalog organization

### Music

Music is an equally important part of Grimoire. The platform is intended
to support:

-   Music library browsing and organization
-   Playback
-   Metadata and discovery
-   User-specific preferences and progress where applicable

### Digital Books

Grimoire also aims to provide an integrated digital reading experience.
Potential supported formats include EPUB and CBZ, with reader features
and customization appropriate to each format.

The specific formats and reader capabilities will be determined during
implementation.

## Distributed Content Sources and CDN

Grimoire is designed around **content sources**: participating hosts or
infrastructure nodes that can store media and, where configured, acquire
and distribute it.

A content source may be operated by the Grimoire platform owner or by a
community contributor. Sources can have different capabilities, storage
locations, media categories, acquisition integrations, and sharing
policies.

### Community-powered delivery

The network is intended to grow as contributors offer storage and
bandwidth. When a viewer requests media, Grimoire can select an
authorized and available source that provides suitable delivery
performance.

Source selection may consider:

-   Network and geographic proximity to the viewer
-   Latency and available bandwidth
-   Source health and availability
-   Whether the source has the requested media
-   Content integrity
-   Access permissions and source-owner policies

The best source for a viewer is not necessarily the source closest to
the original contributor. Grimoire should consider the viewer's delivery
conditions and the availability of suitable copies.

### Source owners retain control

Contributing a source does not make all of its media public. Owners
retain control over source visibility, media sharing, acquisition
permissions, and their own infrastructure.

A node possessing a file does not automatically have permission to serve
it to every user.

## Media Acquisition

In Grimoire, a **media request** can mean requesting that content be
acquired and downloaded to a selected content source. It does not merely
mean asking for permission to watch content that already exists.

Grimoire is intended to coordinate with configurable acquisition
infrastructure, including integrations from the \*Arr ecosystem where
suitable. Potential integrations include:

-   **Sonarr** --- series and anime acquisition
-   **Radarr** --- movie acquisition
-   **Lidarr** --- music acquisition
-   **Prowlarr** --- indexer management for compatible applications
-   **Book acquisition integrations** --- to be evaluated based on
    suitability and maintenance

These integrations are examples of the intended ecosystem; exact support
and configuration requirements remain to be implemented and documented.

### Acquisition workflow

A request is intended to follow a flow similar to this:

1.  A user searches for or selects media in Grimoire.
2.  The user chooses a content source they are allowed to view.
3.  Grimoire checks the user's acquisition policy for that source.
4.  The request is automatically accepted, automatically denied, or sent
    to the source owner for manual review.
5.  An accepted request is passed to the source's configured acquisition
    integration.
6.  The acquired media is processed and indexed.
7.  The media becomes accessible according to the applicable sharing and
    viewing permissions.

**Being allowed to request an acquisition does not automatically grant
permission to view the resulting media.** Acquisition authorization and
media viewing authorization are separate.

## Content Ownership and Permissions

Grimoire is designed to provide explicit access control for content
sources and their media.

### Source visibility

Users may request media from content sources they have permission to
view. Sources that are invite-only or otherwise restricted should not be
exposed to unauthorized users.

Visibility is separate from acquisition permission: a user who can view
a source may still be prohibited from requesting downloads to it.

### Per-user acquisition policies

Content source owners can configure acquisition behavior on a **per-user
basis**. The intended policies are:

  -----------------------------------------------------------------------
  Policy                              Behavior
  ----------------------------------- -----------------------------------
  **Auto-accept**                     Requests from the user are accepted
                                      automatically, subject to
                                      applicable checks and limits.

  **Auto-deny**                       Requests from the user are rejected
                                      automatically.

  **Manual approval**                 Each request is presented to the
                                      source owner for approval or
                                      rejection.
  -----------------------------------------------------------------------

The default policy is **auto-deny for everyone**. Source owners may
explicitly increase or change an individual user's permissions at their
discretion.

This default is especially important for private and invite-only
sources: they should not be bombarded with acquisition requests merely
because a user has been granted viewing access.

### Additional safeguards

The platform is expected to account for protections such as:

-   Request rate limiting
-   Duplicate request detection
-   Owner-defined quotas and resource limits
-   Request and decision audit trails
-   Permission changes and revocation
-   Server-side authorization enforcement

Exact limits and workflows remain open design decisions.

## Identity and Architecture

### Centralized identity initially

Grimoire will initially use centralized authentication and identity
management operated by the platform owner. This keeps account security
and administration manageable for the initial public-facing service.

The architecture should preserve the option to explore federated or
decentralized identity in the future. Decentralized authentication is
not an initial requirement and should not be prematurely implemented.

### Control plane and data plane

Grimoire's architecture is intended to distinguish coordination from
media delivery.

**Control plane --- initially centralized:** - Authentication and
identity - User profiles and preferences - Authorization coordination -
Catalog and metadata coordination - Content-source discovery and
configuration - Acquisition request coordination - Administrative
functions

**Data plane --- designed for distribution:** - Media storage - Media
delivery and caching - Source selection - Media transfer - Potential
distributed processing in the future

This separation should allow the platform's coordination services to
remain manageable while the community-powered delivery network grows.

The current implementation uses React/TypeScript/Vite, Rust/Axum and PostgreSQL.
Distributed source protocols and the deployment/service topology remain open
design work.

## Security Principles

Grimoire is intended to be publicly available, so security is a
foundational requirement.

The design and implementation should prioritize:

-   Least-privilege access for users, services, and contributors
-   Server-side authorization checks
-   Secure authentication, sessions, and tokens
-   Explicit access policies and fail-closed authorization behavior
-   Protection of private media and origin resources
-   Safe handling of acquisition credentials and secrets
-   Abuse prevention and resource-exhaustion safeguards
-   Auditable security-sensitive actions
-   Treating community-operated content sources as untrusted by default

The platform is expected to use a suitable protected-media delivery
mechanism, potentially involving short-lived, resource-scoped access
credentials and CDN-side validation. The specific mechanism, token
lifetime, and revocation behavior are still design decisions.

Grimoire can enforce access controls within its own services and
delivery network, but it cannot guarantee that an authorized viewer will
not record or redistribute content after receiving it.

## Scalability and Resilience

Grimoire aims to scale through participation rather than requiring all
storage and delivery capacity to come from one operator.

The architecture should account for:

-   Sources joining and leaving the network
-   Partial failures and source unavailability
-   Distributed content delivery
-   Horizontal expansion of services where appropriate
-   Content integrity
-   Media availability and potential replication
-   Resource constraints on community contributors

Decentralization and scalability are related but distinct goals.
Grimoire should pursue both with pragmatic architecture, avoiding
unnecessary complexity and premature service decomposition.

## Project Scope

### In scope

-   Unified web-based multimedia consumption
-   Video, music, and digital books as first-class media
-   User accounts and centralized authentication initially
-   Distributed, community-contributed media storage and delivery
-   Content-source discovery and access policies
-   Configurable media acquisition integrations
-   Per-user, per-source acquisition permission management
-   Secure media access and delivery
-   A modular and scalable platform architecture

### Not an initial requirement

-   Decentralized user authentication
-   Decentralized platform governance
-   A finalized microservice architecture
-   A finalized technology stack
-   A guarantee that every content source is always online
-   Unrestricted public access to contributor media

These may be revisited as the project develops, but they should not be
assumed.

## Planned Architecture Considerations

The following topics will be decided as development and design progress:

-   Frontend framework and UI architecture
-   Backend language and framework
-   Database technology and data model
-   Monolith versus service decomposition
-   Content-source registration, discovery, and trust
-   CDN protocols and source routing
-   Media replication and availability guarantees
-   Media access tokens, revocation, and CDN validation
-   Acquisition adapters and supported integrations
-   Metadata providers and catalog strategy
-   Storage, bandwidth, and acquisition quotas
-   Deployment, observability, backups, and disaster recovery
-   Future identity federation

Further technology choices should follow project requirements, maintainability,
security and operational needs while respecting the established implementation.

## Development Status

The local application implements the following capabilities. The vision sections
above also describe the broader platform being developed.

| Area | Available now | Details |
| --- | --- | --- |
| Accounts | Public username/password registration, PostgreSQL sessions, password changes, session activity/revocation, local operator commands | [Authentication](docs/AUTHENTICATION.md) |
| Sources | Administrator-connected local directories, owner-managed visibility/media grants and separate stored acquisition policies | [Source permissions](docs/SOURCE_PERMISSIONS.md) |
| Video | Protected byte-range streaming, browser controls, retry, saved position, sidecar/embedded MKV text subtitles with libass ASS rendering/fonts | [Subtitles](docs/SUBTITLES.md), [progress](docs/PROGRESS.md) |
| Music | Browser playback, queue from visible files, shuffle/repeat, retry and saved position | [Media pages](docs/MEDIA_PAGES.md) |
| Books | Sandboxed EPUB/CBZ readers, chapter titles, text/theme/width controls and saved chapter/page | [Media pages](docs/MEDIA_PAGES.md), [progress](docs/PROGRESS.md) |
| Catalog | For you/Continue, Discover, personal library, search/filter/sort over loaded files and album/series folders | [Media groups](docs/MEDIA_GROUPS.md) |
| Metadata | Embedded extraction, reviewed online matching, genre tags, group covers/tags, stoppable local scans and optional Arr library metadata | [Metadata](docs/METADATA.md), [Arr](docs/ARR_INTEGRATIONS.md) |
| Operations | CI, liveness/readiness, bounded graceful shutdown, manual database backup helper and tested restore procedure | [Operations](docs/OPERATIONS.md), [backups](docs/BACKUPS.md) |

The next major product work needs decisions about upload storage/quotas/sharing,
contributor-node trust/routing, acquisition approval/resource policies and public
deployment. Live Arr/provider validation also requires operator service setup.
The written [follow-up list](docs/AUTONOMOUS_WORK.md)
keeps these decisions separate from completed increments. Media transcoding,
bitmap subtitle rendering, DRM and within-chapter reading-position restoration
are not implemented. The current application is an early prototype; the security
and distributed-delivery requirements remain ongoing engineering work.

See [Dedicated Music library](docs/MUSIC.md) for the Music page, private playlists,
favorites, and fresh-session playback behavior.

## Local Development

The application lives in two projects:

- `web/` is a React and TypeScript single-page app built with Vite.
- `backend/` is a Rust HTTP API built with Axum.

React was selected for contributor familiarity and its established ecosystem.
Vite supplies the development server and production bundler; TypeScript adds
static checks. Axum provides Rust HTTP routing, and PostgreSQL stores identity,
permissions, catalog metadata and personal state. Distributed media delivery and
future service boundaries remain separate architectural work.

The frontend provides video/music playback and EPUB/CBZ reading. Local
username/password accounts support public registration and PostgreSQL-backed
sessions. Sources and media default to private, with explicit per-user source grants; only administrators
may connect directories on the API host. User uploads remain planned as a
separate contribution workflow. For you, Discover, and My library use accessible source files and persistent
per-user saves/open history; see [media pages](docs/MEDIA_PAGES.md).

Before starting the API, follow [account and PostgreSQL setup](docs/AUTHENTICATION.md):
create a dedicated database role, copy `backend/.env.example` to `backend/.env`,
and run `cargo run -- bootstrap-admin YOUR_USERNAME` from `backend/`. Existing
single-user sources can be explicitly imported with
`cargo run -- import-legacy-sources YOUR_USERNAME`. The original JSON is preserved.

Install Node.js with npm and Rust stable with Cargo. CI uses Node.js 24; the
current Vite dependency declares Node.js compatibility as `^20.19.0 || >=22.12.0`.
Follow the PostgreSQL/bootstrap steps above, then run the API in one terminal:

```sh
cd backend
cargo run
```

In another terminal, start the frontend:

```sh
cd web
npm ci
npm run dev
```

Open the local URL printed by Vite. During development, Vite forwards
`/api` requests to the API at `http://127.0.0.1:3000`. The API binds to
loopback by default; deployment binding and frontend/API hosting should
be configured explicitly for the target environment.

Use **Connect directory** in the app and enter an absolute path on the
computer running the Rust API (for example, `/home/you/Media`). Grimoire
recursively lists common video (`mkv`, `mp4`, `webm`, and others), audio
(`mp3`, `flac`, `m4a`, and others), EPUB, and CBZ files. It skips symbolic
links, limits each scan to 10,000 media files and 100,000 directory
entries, and serves selected media and book files through scoped endpoints. Disconnecting a source
only removes its registration; it never deletes files. Source registrations and ownership are stored in PostgreSQL.

Accounts authenticate before accessing any source or media endpoint. New public
accounts start with no source/media access. Owners can configure visibility, media access, and acquisition policy per user
through the [source permissions editor](docs/SOURCE_PERMISSIONS.md). Directory
registration remains a server administrator operation. Public HTTPS hosting, account operations, session
policies, and current limitations are documented in
[Authentication and authorization](docs/AUTHENTICATION.md).

### Music playback

Select a connected source, optionally filter to **Music**, then press **Play**
on a track. The player stays open while you browse and provides play/pause,
seeking, volume, previous/next, and automatic advancement through the music
files visible when playback starts. Shuffle and repeat default to off; the controls
allow repeat-one/repeat-all. A failed stream has an explicit retry. Closing the player or disconnecting its
source stops playback. Restart the Rust API after updating the code.

Audio is delivered through `GET /api/sources/{id}/audio?path=...`, with HTTP
range support for seeking via [tower-http ServeFile](https://docs.rs/tower-http/0.6/tower_http/services/struct.ServeFile.html). Each request
requires a registered source and a relative audio-file path within it;
symlinks and traversal are rejected, and responses disable caching. Each request also authenticates its session and checks ownership or an explicit
source visibility/media grant. Browser
codec support determines which listed audio formats play; there is no
transcoding yet. Local filesystem writers are trusted: files must not be
replaced concurrently with symlinks while requests are being resolved.

### Video playback

Select a connected source and press **Watch** beside a video file. The video
player provides browser controls for play/pause, seeking, volume, and fullscreen.
Use **Back** to leave the watch page; Escape exits fullscreen. Starting video stops music,
and starting music closes video, so only one media item plays at a time.

Video uses `GET /api/sources/{id}/video?path=...` with the same source checks,
path restrictions, byte-range delivery, and private cache policy as audio.
This legacy endpoint accepts only recognized video extensions. The web player
now always uses segmented H.264/AAC transcoding, with source-owner encoder policy
and viewer-selectable resolution/bitrate. Companion subtitle files and
embedded MKV text tracks are available through the subtitle selector. ASS uses
libass with embedded font support; use **Fullscreen with subtitles** for overlays.
See [watch pages, shareable links and MKV audio selection](docs/WATCH_PAGES.md)
and [subtitle setup and limits](docs/SUBTITLES.md). Audio selection now starts segmented H.264/AAC playback. CPU and optional
NVIDIA/AMD encoding are configured by the source owner; player Settings select
quality and display target bitrates. See
[transcoding setup, limits and testing](docs/TRANSCODING.md). Bitmap captions
remain future work. Restart the Rust API after updating.

### EPUB and CBZ reading

Select **Read** beside a book in a connected source. EPUB chapters appear in
spine order with table-of-contents titles, adjustable text size and Paper/Night
themes. CBZ displays raster
image pages in natural filename order, with page selection and a width control.
Use the close button or Escape to return to browsing. Opening a reader stops
music and video playback. Chapter/page positions now persist across sessions; see [progress](docs/PROGRESS.md).

Books use `GET /api/sources/{id}/book?path=...` and the same registered-source,
path, and cache restrictions as the media endpoints. Archives are processed in
browser memory with [zip.js](https://gildas-lormeau.github.io/zip.js/). The initial
limits are 128 MiB compressed, 256 MiB expanded, 32 MiB per entry, and 5,000
archive entries; encrypted archives are rejected. EPUB HTML is sanitized using
[DOMPurify](https://github.com/cure53/DOMPurify), then shown in an iframe with an
empty sandbox and a restrictive content security policy. Scripts, external
resources, forms, and chapter hyperlinks are disabled. EPUB publisher styles
are replaced by the reader's layout; complex fixed-layout books, SVG-only pages,
and DRM are not supported. Named in-book bookmarks and within-chapter scroll
restoration are not implemented; saved chapter/page positions are supported. CBZ supports
JPEG, PNG, WebP, GIF, and AVIF images according to browser support. Corrupt or
unsupported content produces a reader error.

After updating, run `npm ci` in `web/` when dependencies change and restart both
development servers.
Reader checks can be run with `npm test` in `web/` and `cargo test` in `backend/`.

### Personal media pages

Use **Discover** to browse accessible files, bookmark items into **My library**,
and return to recently opened/saved media in **For you**. All three pages respect
current source permissions. [Media pages](docs/MEDIA_PAGES.md) documents search,
filters, sorting, personal-state storage, and current scan limits.

### Metadata and organization

Source owners can gather local tags/covers, edit display titles and genres, and
review online matches from TMDB, MusicBrainz/Cover Art Archive, and Open Library.
Catalog cards show authorized artwork, with genre filters and album/series grouping.
Filenames and bookmarks remain unchanged. See [Metadata](docs/METADATA.md) for setup,
permissions, FFmpeg requirements, provider credentials, and extraction limits.

### Resume playback and reading

Audio/video resume saved timestamps; EPUB/CBZ resume chapters/pages. Positions are
private to the account and require current media access. Concurrent players use
revision checks to avoid silently overwriting one another. See [Progress](docs/PROGRESS.md)
for save timing, failure behavior and current reading-position limits.

### Album/series folders and custom covers

Albums, book series and TV series now open as folders, with season/disc folders
where relevant. Owners can upload a custom group cover and edit tags that apply
to every current/future member in their source. Item credits reflect active
metadata/artwork providers. See [Media groups](docs/MEDIA_GROUPS.md) for usage,
authorization, upload limits and attribution details.

### Optional Arr metadata connections

Owners can connect Sonarr, Radarr and Lidarr per source, test connections, map
folders, and preview metadata for existing Arr library files. Current local
extraction and online matching remain the default; Arr is optional. See
[Arr integrations](docs/ARR_INTEGRATIONS.md) for encryption-key setup, approved
URLs, folder mapping, API versions and operational limits. Download/acquisition
requests remain future work.

## Checks and Operations

From the repository root, the normal local checks are below. Backup-helper tests
use Linux/GNU tools, matching CI.

```sh
npm --prefix web ci
npm --prefix web test
npm --prefix web run build
cargo fmt --manifest-path backend/Cargo.toml --check
cargo clippy --locked --manifest-path backend/Cargo.toml --all-targets -- -D warnings
cargo test --locked --manifest-path backend/Cargo.toml
python3 -m unittest discover -s scripts/tests
```

PostgreSQL tests use a dedicated disposable role/database, not the live app's
connection. SQLx needs permission to create its temporary test databases. The
ignored test suite also includes a synthetic MKV/ASS/font extraction check and
requires FFmpeg, FFprobe and DejaVu Sans (`fonts-dejavu-core` on Ubuntu). From
`backend/`, with the test connection explicitly supplied:

```sh
DATABASE_URL='postgresql://TEST_ROLE:TEST_PASSWORD@localhost/TEST_DATABASE' cargo test --locked -- --ignored
```

See [authentication testing](docs/AUTHENTICATION.md), [subtitle requirements](docs/SUBTITLES.md)
and the [CI workflow](.github/workflows/checks.yml). Operational guidance covers
[shutdown/readiness](docs/OPERATIONS.md) and [manual backups/recovery](docs/BACKUPS.md).
Production database roles, TLS/origin configuration, secret storage, media backup
and deployment settings must be established for the target environment.

### Site branding

The sidebar logo and browser icons use `Data/Assets/Grimoire_Icon.svg`.
The original artwork is preserved. Browser favicons have transparent
backgrounds; the sidebar and Apple touch icon use a lavender background. Web assets live in
`web/public/` and are included by Vite. To regenerate the SVG copy, PNG icon,
Apple touch icon, and multi-size ICO after changing the source, run:

```sh
python3 scripts/generate_brand_assets.py
```

The conversion script requires Pillow and supports the embedded PNG layers
in the current SVG. It fails if the source changes to an unsupported structure.


## Contributing

Contributions, ideas, and discussions are welcome as the project takes
shape.

Before proposing major changes, please consider Grimoire's guiding
principles:

1.  Video, music, and books are equally important.
2.  Content-source owners retain control of their infrastructure and
    media.
3.  Acquisition permission defaults to deny.
4.  Viewing, acquisition, and media access are distinct permissions.
5.  Media delivery should be distributed where possible.
6.  Authentication is centralized initially, with future flexibility.
7.  Public-facing security is foundational.
8.  Architecture should scale without needless complexity.

Please consult the repository's `AGENTS.md` for additional project
scope, engineering guidance, and current open design questions.

## Responsible Use

Grimoire is infrastructure for managing, acquiring, and delivering
multimedia. Users and operators are responsible for ensuring that the
media they acquire, host, share, and distribute is handled in accordance
with applicable laws, licenses, and rights-holder permissions.

The platform's access-control features are intended to respect
contributor choices and do not themselves establish rights to any
content.

## License

A project license has not yet been specified. Licensing information will
be added once the project owner selects a license.
