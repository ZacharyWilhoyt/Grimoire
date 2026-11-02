# Autonomous development work

The owner authorized continued work on well-defined backlog items while away.
Existing defaults, ownership boundaries and user changes must be preserved.
Completed increments are tested, committed and pushed individually.

## Completed

- Automated GitHub checks for the frontend production build/tests and Rust format,
  strict Clippy, unit tests and PostgreSQL integration tests. CI uses an isolated,
  disposable PostgreSQL service and requires no production credentials.

## Next work

- Persistent playback and reading progress across sessions/devices, with the same
  per-user isolation and current media-access checks as library/activity state.
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
