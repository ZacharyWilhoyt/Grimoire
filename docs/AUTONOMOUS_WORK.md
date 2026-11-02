# Autonomous development work

The owner authorized continued work on well-defined backlog items while away.
Existing defaults, ownership boundaries and user changes must be preserved.
Completed increments are tested, committed and pushed individually.

## Completed

- Automated GitHub checks for the frontend production build/tests and Rust format,
  strict Clippy, unit tests and PostgreSQL integration tests. CI uses an isolated,
  disposable PostgreSQL service and requires no production credentials.

## Current status

The initial CI/progress work and subsequent completed increments are recorded
below. Continue and saved-position indicators are implemented and browser verified
for all media types. The remaining major product decisions are listed next.

## Deferred decisions / owner follow-up

- User-upload storage/resource quotas and ownership/sharing policies: choose limits
  and confirm the contribution model before exposing arbitrary media uploads.
- Distributed contributor-node registration, trust and routing protocol.
- Acquisition approval lifecycle, quotas and book acquisition integration.
- Real Arr/TMDB verification needs the owner's configured services/credentials;
  existing simulated-service tests do not claim live-instance compatibility.
- Public deployment needs its hostname, TLS/reverse proxy and backup destination.

No credentials should be written here. Do not include AGENTS.md in commits: it was
already an untracked owner-provided file before this work started.

## Progress implementation status

The backend now exposes session-protected GET/PUT `/api/progress`, keyed to the
current user and an authorized source/path. Positions use seconds for audio/video
and zero-based chapter/page indexes for books, with an optional book locator.
Writes require the revision returned by the preceding read; conflicting players
receive 409 instead of silently overwriting each other. The client is now wired to audio/video time and book chapter/page restoration.
Build/unit tests pass. Browser verification confirmed CBZ page and EPUB chapter
restoration after closing/reopening, audio restoration beyond a seeded 20-second
position, and video restoration beyond 18 seconds. Database reads verified saves. Completion rules remain deferred
rather than inventing an arbitrary percentage threshold.

## Additional completed increments

- Continue catalog and saved-position labels, including source revocation tests
  and browser verification for video/music/EPUB/CBZ saved positions.
- Password-confirmed sign-out of other account sessions, preserving the caller and
  other accounts; tested against disposable PostgreSQL.
- GitHub Actions runs for the CI/progress storage and player-resume increments were
  independently observed completing successfully through GitHub's public API.

## Music queue controls

Shuffle and repeat controls are implemented with unchanged off/off defaults.
Queue selector and DOM component tests cover stable shuffle ordering, repeat
boundaries, visible control states, native repeat-one and single-track repeat-all.
Production build and all frontend tests pass.

## Subtitles

Added sidecar SRT/WebVTT/ASS/SSA and embedded MKV text-track selection, with libass
WASM for unflattened ASS and embedded TTF/OTF attachments. Added a fullscreen
wrapper after browser testing caught native video fullscreen omitting the canvas.
Synthetic MKV verification confirmed positioned text, vector drawing, italics,
karaoke and corrected fullscreen alignment. Authorization/revocation, original
script preservation and byte-exact font extraction have regression coverage.
See SUBTITLES.md for processing limits and remaining bitmap/transcoding work.

## Progress edge cases

A delayed progress lookup now respects a pause made before restoration instead of
starting playback again. CBZ pages are recorded only after the browser image load
succeeds, so damaged images do not overwrite the previous reading position.
Regression tests cover delayed lookup, early seeking, image success/failure and
retain the existing file-switch save test. All 36 frontend tests and build pass.

## EPUB chapter titles

EPUB3 navigation documents and EPUB2 NCX titles now label the chapter selector.
Reading order and saved archive locators remain based on the package spine.
Malformed/missing optional navigation falls back to numbered sections, and TOC
links cannot add off-spine or external resources. Parser and reader integration
tests cover titles, nested entries, encoded paths, fallback and saved locators.
All 40 frontend tests and the production build pass.

## Permission roster failure handling

A failed permission-list request no longer looks like an empty grant roster or
claims the source is owner-only. The editor now requires a successful retry
before changing grants. Successful mutations remain reported as applied when a
subsequent roster refresh fails, and further editing is blocked pending reload.
Regression tests cover initial failure/retry and revocation followed by refresh
failure. Server-side authorization remains unchanged.

## Independent home-section loading

For you now attempts Continue, history, saved items and source discovery
independently. One failed endpoint no longer prevents healthy sections from
loading; errors identify the affected section. Requests remain sequential to
respect server scan limits, and navigation aborts stop subsequent requests and
discard late responses. Loader and full App rendering tests cover partial failure,
page-specific endpoints and cancellation. All 46 frontend tests and build pass.

## Bounded API shutdown

Added SIGTERM (Unix) and Ctrl+C draining: stop accepting connections, allow active
requests up to 30 seconds, then close lingering streams. Existing unit tests and
Clippy pass; runtime checks use only the disposable preview database and synthetic
media. See OPERATIONS.md for supervisor timing and the distinction between active
HTTP requests and non-durable background metadata jobs.

## Database readiness

Added session-free `/api/ready` alongside existing liveness. It checks only the
required control-plane database, with a two-second deadline and two-probe
concurrency bound, generic response data and no-store caching. PostgreSQL
integration coverage checks healthy/saturated/recovered/closed pools and verifies
liveness stays independent. Individual community sources are not readiness gates.

## EPUB reading theme

Added a session-only Night theme beside the existing text-size control, preserving
Paper as the default and leaving comic image colors unchanged. Theme choices use
fixed palettes, keep the chapter CSP/sandbox intact, and do not rewrite progress.
Documented the existing chapter-redraw behavior of text appearance controls.
All 47 frontend tests and the production build pass.

## Media retry and ASS overlay cleanup

Audio/video failures now offer explicit retry without leaving the player. Retry
recreates the media element and progress session, rechecks saved position, and
preserves volume/mute/speed. Existing revision conflict protections remain active.
ASS overlays are explicitly released even when their old video element was
already detached, covering retries and in-place video replacement. Hook and DOM
regressions cover restored settings, retry actions and detached-overlay cleanup.
Subtitle delay is also preserved when the video element is replaced; delayed
renderer startup no longer attempts to resize a renderer before it is ready.
All 51 frontend tests and the production build pass.

## Owner-controlled scan stopping

Added Stop scan for the current local metadata job, scoped by source and job ID
with owner/Origin/CSRF enforcement. Uses the existing interrupted state and keeps
completed metadata/source files. Workers check before further extraction and do
not overwrite terminal job states. Tests cover non-owner administrators, wrong
source/job, missing CSRF, idempotent stops, terminal records, queued extraction
cancellation, source-byte preservation and UI state refresh. All 19 PostgreSQL/
extraction tests and 52 frontend tests pass; Clippy and build pass.

## Metadata status recovery

Added a read-only Retry scan status action for initial and polling failures.
Unknown/last-known status no longer enables scan mutations, and pending status
reads/timers are canceled on navigation. Status errors are separate from mutation
errors. Initial failure and timed-poll failure recovery tests pass along with all
54 frontend tests and the production build.

## Dialog and keyboard focus

Connect-directory now uses a native modal dialog with explicit input focus,
Escape/backdrop dismissal and opener-focus restoration. Global Ctrl/Cmd+K leaves
open dialogs and unrelated editable controls alone, while remaining available in
the workspace/search field. Unit/DOM tests and production-browser verification
cover the lifecycle and keyboard behavior. No directory was registered during
browser testing. All 56 frontend tests and the build pass.

## Manual backup helper and recovery drill

Added an explicit-service PostgreSQL custom-dump helper with private temporary/
final files, archive validation, atomic no-overwrite publication and failure
cleanup. No production database was backed up or restored. A real restore drill
on the isolated disposable cluster compared all 19 public table row counts/data
checksums and all 3 sequences; backup permissions and refusal to overwrite also passed. Added
credential-free safety tests to CI. Scheduling, encryption/off-site destinations,
retention, production service profiles and real-media recovery remain owner work.
See BACKUPS.md, including security-state reconciliation after restoring old data.

## Shared native-dialog lifecycle

Reader, video, account, source permissions, metadata, group and directory dialogs
now share one lifecycle hook. Data reloads no longer close/reopen a modal; cleanup
uses the captured element even after React clears its ref and restores a connected
opener without scrolling. Book archive cleanup separately handles its async close.
Regression tests cover focus/edit retention across updates, disconnected openers,
and permission retry without reopening. All 58 frontend tests and build pass.

## Login issuance race hardening

Found and closed a verification-to-issuance race: a login that verified the old
password could otherwise create a session after a password change revoked prior
sessions. Issuance now locks/rechecks the current user/hash/disabled state,
serializes concurrent session pruning, and commits successful-login auditing with
the session. Deterministic PostgreSQL tests observe the real login waiting on that
lock before applying password-reset/disable transactions, then assert generic 401,
no session/cookie and failed-login auditing. Parallel logins retain at most ten
sessions. Account flows and owner/source policies remain unchanged.
Strict Clippy, 22 Rust unit tests and all 21 database/extraction tests pass after
the issuance change, including existing rotation/password-change/default-deny
regressions. No schema migration or public authentication API change is required.

## Private session activity view

Added a read-only active-session view to account settings, showing current/other
session times before the existing password-confirmed revocation action. The
protected endpoint is caller-scoped, expiry-filtered, bounded to ten and exposes
only timestamps/current-session status. Account actions refresh the snapshot;
failures have a read-only retry and aborted reads cannot overwrite later data.
Verified production layout, refresh and keyboard focus on disposable sessions.
All 62 frontend tests, 22 database/extraction tests, Clippy and build pass. Remote
CI independently confirmed the preceding login-race fix and modal/backup commits.

## Current implementation documentation

Updated the main README to separate implemented capabilities from the broader
vision and open owner decisions. Removed stale claims that PostgreSQL, subtitles
and saved reading progress were undecided/unavailable. Added a capability/guide
index, reproducible local checks and the current ignored-test prerequisites.
Local documentation links and branding paths resolve; this increment changes no
runtime behavior.
