# Federation implementation checklist

Scope comes from the Federated Backend and Source Agent Implementation Specification. This plan uses the current Grimoire checkout at `46916c0` as the compatibility baseline.

## Phase 0 repository audit

- **API and identity.** `backend/src/main.rs` mounts session-protected source, catalog, playback, metadata, history, music, comments and administration routes. Browser mutations use the origin and CSRF checks in `auth.rs`. Global access rules and trusted proxy handling live in `security/`; the frontend uses same-origin requests in `web/src/api.ts`. Node credentials need a separate authentication boundary within the instance's network policy.
- **Storage and delivery.** Source registration is restricted to admins. `sources.path` is required and unique, and its CHECK permits only `local_directory`. `media_root` combines source authorization with filesystem resolution. `catalog.rs` performs bounded local scans, and byte serving in `main.rs` uses the existing path and range checks. `transcode.rs`, `video_audio.rs`, `subtitles.rs`, metadata and sidecar extraction expect local filesystem access.
- **Identifiers.** Current media keys hash the source ID and relative path. Progress, saved history, music references, metadata and comments depend on those keys. Keep existing IDs and key derivation for local media. Remote identity needs opaque, stable IDs for items, representations and locations so a rename can update a label without rekeying activity.
- **Permissions.** Source ownership, discovery, media access and acquisition are established in `permissions.rs` and the source tables. Some existing catalog routes intentionally require both discovery and media access. Preserve local behavior and keep agent-local deny authoritative for remote requests. Policy projections need an acknowledged revision and fail-closed behavior when stale or unverifiable.
- **Operations.** The current deployment uses Caddy, an API container, PostgreSQL via a private Unix socket, and a private managed Arr controller. The API runs SQLx migrations at startup, supervises Caddy settings, and starts metadata scans and audio/transcoding work on demand. The node lifecycle must use bounded work and isolate failures from local playback.
- **Snapshot differences.** The spec references migrations through `0014`. The checkout is at `0018` and includes managed Arr, series synopsis and season artwork, comments and moderation, and a local history-repair tool. The next migration is `0019` if the branch has no intervening migration. Keep those features and historical migration checksums. Preserve the operator's existing Compose bind mounts.

## Phases and file plan

- [x] **0. Audit and baseline.** Record the checks and the pre-existing Clippy finding below.
- [x] **1. Provider seam and additive schema.** Add `backend/src/sources.rs` for authorized source lookup and local listing, wire it into `main.rs`, and move one read-only catalog path in `catalog.rs` through the local provider first. Keep the range-serving implementation intact. Add the node registry, source-kind constraints and node binding in a new migration under `backend/migrations/`. Preserve the unique local path constraint and enforce the remote/local shape in the database. Add an upgrade fixture in `auth_integration.rs` that verifies existing rows and identifiers after migrating. Adopt the seam in other routes only when they need it.
- [ ] **2. Identity and enrollment.** Add a separate Rust agent binary and shared versioned wire types under `backend/src/`. Add coordinator node registration, enrollment and heartbeat routes to `main.rs` with node-specific authentication. Define the wire contract in `docs/NODE_PROTOCOL.md` and operator setup in `docs/SOURCE_AGENT.md`. Test short-lived, single-use enrollment; owner binding; key replacement and revocation; replay handling; and incompatible protocol versions.
- [ ] **3. Remote index and policy.** Add catalog persistence for remote items, representations and locations. Integrate a remote catalog provider in `catalog.rs`; adapt `permissions.rs`, metadata and artwork lookups, and the permission documentation to use acknowledged policy revisions. Keep local Arr behavior and acquisition permissions intact. Test incremental replay, partial scans, tombstones, offline sources, and leakage through search, artwork, activity and comments.
- [ ] **4. Delivery.** Add a coordinator delivery broker and agent range handler, with playback grants scoped to the node, representation and operation. Introduce an outbound authenticated relay with per-node and per-session budgets so a NAT-bound agent can serve media. Add direct HTTPS for verified endpoints. Keep the agent's policy check on each request and ensure client IP policy remains effective on both routes. Update the media playback components and API client only where remote URLs or capabilities need an additive contract. Cover seeking, range requests, cancellation, books, subtitles, expiry, local deny, relay limits and disconnects.
- [ ] **5. UI and operations.** Add enrollment, status, local-root setup instructions, policy acknowledgments and diagnostics to the existing source screens. Extend `Dockerfile`, `deploy/`, `docs/DEPLOYMENT.md` and the backup/recovery guidance with an independent agent deployment and key lifecycle. Add deployment tests alongside `scripts/test-compose-deployment.py`.
- [ ] **6. Compatibility gate.** Run the full frontend, Rust, PostgreSQL, proxy and Compose suites plus end-to-end direct and NAT-bound playback tests. Validate migration from the deployed schema with local permissions, progress, metadata, Arr and comments intact. Document the future hybrid distribution model in `docs/CDN_ARCHITECTURE.md`.

Delivery, trust and revocation details belong in the protocol design before remote playback is enabled. In particular, browser credentials need to stay on the coordinator origin, and any direct route needs a way to preserve instance-wide IP policy and agent-local revocation. A relay carries media bytes through central infrastructure and must be budgeted and observable. CDN placement, replication and distributed transcoding remain deferred.

## Baseline checks

- Frontend Vitest suite and production build passed. 118 tests ran.
- Rust formatting and the backend unit suite passed. 33 unit tests ran.
- All 36 ignored backend tests passed against disposable PostgreSQL 18 with Caddy 2.11.8 configured. The first run lacked the Caddy binary setting, and the Caddy test passed after setting it.
- All 17 operations unit tests, the Caddy proxy smoke test, and CPU Compose configuration validation passed.
- Strict Clippy failed on the existing managed Arr test in `backend/src/metadata/arr/managed.rs`. It flags the test's mutex guard as held across asynchronous fixture cleanup. Fix that baseline issue separately before the Phase 1 compatibility gate.
- The full Docker deployment suite and remote-node acceptance tests remain Phase 6 checks. The live deployment was not changed during this audit.

## Phase 1 compatibility gate

Migration `0019` adds the registry and a database-enforced owner match between a remote source and its node. The existing local path constraint, local IDs, and permission records stay in place. The local source listing now uses `LocalSource`, and filesystem-only routes exclude remote rows until a remote provider is wired in. Existing byte-range serving is unchanged.

The pre-existing Arr test lock was scoped to its assertions. Formatting, strict Clippy, 33 unit tests, and 37 integration tests passed. The upgrade fixture applied migrations `0001`–`0018`, seeded a local source, grant and progress, then applied the current migrations twice and verified that the records remained usable.
