# Unified catalog Phase 0 audit

This audit covers the deployed Source Agent checkout at `09b0991` and uses
`GRIMOIRE_UNIFIED_FEDERATED_CATALOG_SPEC.md` as the behavioral specification.
GitHub `main` was at `eff78ee` at the start of the audit. It contains migrations
through `0014`. The operational branch contains migrations through `0021`,
including managed Arr, comments and the Source Agent. The branches have
diverged by three main-only and 26 operational-branch commits. The Phase 0
push adds this document to main. Before Phase 1, merge the operational baseline
into main, resolve and test the combined tree, and verify applied migration
checksums against a deployment copy before adding a migration.

## Current data and request paths

| Area | Current behavior and dependencies |
| --- | --- |
| Local ingestion | `catalog.rs` scans authorized local sources on demand and caps aggregate discovery at 32 sources and 5,000 files. `sources.rs` and the filesystem resolver in `main.rs` handle source-specific browsing and path checks. There is no complete persistent local inventory. `media_metadata` is enrichment for file keys and may be absent for valid files. |
| Remote ingestion | `nodes.rs` authenticates enrollment and heartbeats. The agent indexes approved roots and journals signed deltas. `remote_catalog.rs` validates node ownership, revision and digest, then applies changes and policy projections transactionally. Migration `0021` separates item, representation and location tables, but all IDs are source-scoped and each item currently has one representation. Deltas carry opaque UUIDs, a basename, extension and size. The agent fingerprint is local and does not prove cross-source byte identity. |
| Metadata | Local extraction, manual overrides, sidecars and reviewed matches enrich existing file keys. TMDB TV IDs can identify a series even when a file represents an episode. MusicBrainz searches recordings and Open Library uses work IDs. Sonarr and Lidarr match identifiers may be scoped to a specific Arr connection and file or track. `extract_stamp` records size and modification time. Provider IDs and stamps therefore need typed provenance before they can establish an identity. |
| Permissions | Local catalog pages generally require discovery and media access together, while the source roster can expose a source with discovery permission alone. Remote catalog reads intersect the central grant and the agent's projected grant. Relay reads also recheck playback access, browser session, liveness and node status, and the agent rechecks its live local policy before each chunk. Acquisition remains a separate permission. |
| Offline and deletion | Remote catalog and relay reads both require a heartbeat and catalog confirmation within 90 seconds. Rows survive a disconnect, but the remote catalog hides them. The v1 `published` flag represents both staged batches and intentional unpublishing. Explicit item tombstones delete source-local index rows and cascade to their representation and location. |
| Frontend and history | `catalogLoader.ts` uses `/api/catalog`, `/api/library`, `/api/activity` and `/api/continue`. The existing library is a saved-items view. Music grouping and queues, saves, progress, comments, artwork, series links and statistics depend on the existing source ID, path, file key or group ID. `SourceAgents.tsx` browses a separate API. Its media URLs bypass the normal open-history flow, and the remote book reader currently skips local progress persistence. |
| Playback | Local audio, video and book routes use authorized filesystem paths. Video transcode, subtitle and audio-track endpoints have local processing and capability checks. Remote playback uses the existing signed range relay in `relay.rs` and browser-compatible representations. There is no common resolver or automatic failover. |

## Compatibility risks and proposed decisions

- **Identifiers and history.** Keep local file keys derived from source and
  path, remote source UUIDs and representation UUIDs, and existing group IDs.
  Add stable logical IDs and aliases above them. Keep comments, Arr bindings,
  artwork overrides and original activity rows attached to their existing
  records. Aggregate personal history for display, preserving its original
  provenance. Use the latest valid position on a compatible timeline and keep
  conflicting or edition-specific positions available for recovery.
- **Identity and provenance.** Add typed evidence and reversible associations.
  Use verified provider/entity/edition matches for automatic links and hashes
  only for byte identity after validating the media type. Generate weak
  candidates without automatically linking them. Scope routine suggestions and
  undo to the source owner, with bounded, deduplicated queues and curator review
  for disputes or global changes.
- **Visibility.** Apply discovery permission before aggregating cards,
  metadata, artwork, search facets, counts and suggestions. Preserve existing
  local permission behavior in legacy routes. Keep remote offline metadata
  visible only within a bounded, last-confirmed policy window. Central revoke,
  explicit unpublish, node revoke and uncertain policy state fail closed.
  Because v1 conflates staging and unpublishing, conservatively hide both
  until an additive protocol field can distinguish them.
- **Compatibility and routing.** Keep existing transports and local feature
  endpoints. Add capability-aware resolution over authorized locations,
  using cached measurements and quality preferences. Reauthorize on every
  selection and failover. Reuse byte offsets only for verified identical
  representations and timestamps only for compatible timelines. For books,
  use a verified compatible edition and locator. Keep source-specific edits
  and subtitles attached to the selected source or representation.
- **Performance and recovery.** Backfill in bounded batches and keep normal
  local browsing usable before indexing completes. Project from stored state
  without agent round trips. Use indexed joins and stable logical pagination,
  with fixture-based query plans and rollback tests. A deleted location
  leaves the logical identity and retained references intact.

## Implementation and acceptance gates

1. **Catalog foundations.** Add the next migration after the reconciled
   baseline, expected to be `0022`. Add logical identity, typed evidence,
   associations and aliases. Reuse existing remote source records as physical
   inputs and add an incremental local inventory bridge. Verify a populated
   upgrade with unchanged deep links, progress, playlists, comments, Arr and
   local playback. Keep legacy response shapes available.
2. **Remote ingestion.** Project the accepted signed deltas into the unified
   inventory at the transaction boundary. Leave existing v1 agent operations
   intact. Test first sync, replay, interrupted publication, tombstones,
   reconnect, revocation, offline visibility and bounded staleness.
3. **Matching and deduplication.** Implement verified type-aware matching and
   source-owner review with audit and undo. Test distinct editions, remakes,
   episodes, recordings and releases, forged evidence, source isolation,
   declined suggestions and concurrent updates.
4. **Unified pages.** Add permission-filtered logical projections to Discover,
   For You, saved My Library entries and Music. Route series, details and
   readers through compatibility adapters. Verify one card per eligible logical
   item and preserve filters, queues, navigation, recommendations and history.
5. **Playback routing.** Add the resolver, measurements, eligible manual
   override and bounded failover. Test source failure before playback and
   mid-stream, revoked permissions, quality and capability selection, and
   progress continuity across compatible copies.
6. **Verification.** Run frontend, backend, upgrade, relay, Arr, proxy and
   Docker regression suites. Verify real browser playback and seeking, private
   duplicate isolation, query performance and deployment rollback. Keep the
   representation/location separation available for future CDN copies.

Phase 0 changes documentation only. The live installation and application
schema were not changed.

## Baseline checks

On the operational checkout, all 120 frontend tests and the production
frontend build passed. All 34 coordinator unit tests, two agent tests and
39 ignored integration tests passed with disposable PostgreSQL and Caddy.
These checks establish the existing behavior before implementation. The
unified catalog, matching and failover acceptance tests belong to the later
phases above.
