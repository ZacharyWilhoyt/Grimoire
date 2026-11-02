# Autonomous development work

The owner authorized continued work on well-defined backlog items while away.
Existing defaults, ownership boundaries and user changes must be preserved.
Completed increments are tested, committed and pushed individually.

## Completed

- Automated GitHub checks for the frontend production build/tests and Rust format,
  strict Clippy, unit tests and PostgreSQL integration tests. CI uses an isolated,
  disposable PostgreSQL service and requires no production credentials.

## Next work

- Continue section and saved-position indicators are now implemented and browser
  verified for video, music, EPUB and CBZ; source revocation/user isolation tests pass.
- Other small, reversible usability improvements supported by the existing design.

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
