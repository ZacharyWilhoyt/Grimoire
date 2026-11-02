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
