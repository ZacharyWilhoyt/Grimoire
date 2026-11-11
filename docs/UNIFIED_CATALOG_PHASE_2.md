# Unified catalog Phase 2 remote ingestion

Accepted v1 Source Agent deltas now project remote representations into the
unified inventory in the same transaction as the existing agent index. A
confirmation can backfill up to 256 already-indexed representations for that
source in addition to projecting the current upserts. Exact retries of an
accepted revision return the existing result. A metadata-only rename keeps
the logical ID. A change to reported format or size gets a new logical item
and representation until matching can establish compatibility. Tombstones
remove physical locations and retain the logical records.

The source-local index remains the provenance for the basename, format and
size. No agent paths, credentials or unverified global identity claims enter
the catalog. Existing Source Agent binaries and the public catalog, agent
and relay response contracts are compatible with this phase.

Migration `0023` adds the internal `catalog_remote_discovery` view. It
combines current central discovery permission with the last acknowledged
agent grant and excludes unpublished sources, revoked nodes and disabled
accounts. The catalog remains eligible for discovery for five minutes
after the last successful catalog confirmation, provided those permissions
still hold. A heartbeat or catalog confirmation older than 90 seconds marks
the location unavailable for playback. The existing relay independently
reauthorizes the request. The five-minute policy window is a Phase 2
default and is separate from the playback lease.

The view is ready for the unified page work in a later phase. Current
frontend pages still use their existing catalog endpoints. No production
migration was applied for this checkpoint.

## Verification

Strict Clippy, 36 Rust unit tests and 39 integration tests passed. The
integration fixture covers confirmation-only backfill, repeat delivery,
offline visibility, discovery without playback access, lease expiration,
explicit unpublishing, revocation and tombstones. The HTTPS smoke test
passed with enrollment, indexing, range playback, re-enrollment and
agent-local denial.
