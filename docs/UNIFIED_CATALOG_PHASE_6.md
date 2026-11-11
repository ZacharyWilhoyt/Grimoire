# Unified catalog verification and recovery

## Verification

| Area | Result |
| --- | --- |
| Rust | Formatting and strict Clippy, 37 unit tests and 42 PostgreSQL integration tests |
| Frontend | Production build and 123 tests |
| Operations | 17 Python tests and CPU and NVIDIA Compose configuration validation |
| HTTPS | Disposable Caddy proxy and live Source Agent smoke tests |
| Upgrade | PostgreSQL schema 0018 to 0025, including a second migration run and playback using existing local IDs, metadata, bookmarks, progress and permissions |
| Restore | Control-plane dump restored to a separate disposable database and compared by row count and digest across 43 public tables |

The live agent check enrolls and re-enrolls an agent, opens its unified
catalog entry, saves a bookmark and progress, reads an exact multi-chunk
range, checks agent-side denial, and fails over to an authorized local
encode after revocation. The signed-agent integration test also covers
discovery-only access, offline visibility, artwork protection, and removal
from personal pages after access is revoked. Unified relay URLs now bind
the requested catalog representation version. A material replacement
invalidates older URLs even if an agent reuses its wire identifier.

Compose configuration was validated locally. The full Compose deployment
smoke is CI-only. This checkpoint did not start containers against the live
site, exercise production GPU devices, or validate public TLS issuance.

## Catalog benchmark

Run with the installed PostgreSQL test binaries from the repository root:

```sh
GRIMOIRE_TEST_POSTGRES_BIN=/usr/lib/postgresql/18/bin scripts/benchmark-unified-catalog.sh
```

The script starts an isolated PostgreSQL cluster on a private Unix socket and
populates 2,000 logical items with 8,000 locations across four sources. One
remote source is offline and another is private. The viewer saw 4,000 remote
locations, of which 2,000 were playable. On the development host, the remote
projection count took about 78 ms, a 31-row logical page took about 59 ms, and
a single-title playable location lookup took about 42 ms. These are SQL
fixture timings on one host. They exclude filesystem scans, application
serialization, browser rendering and concurrent production load.

## Rollout and recovery

1. Record the deployed commit, database name, and image digests. Take and
   verify a protected control-plane backup using [Backups](BACKUPS.md).
   Keep the original Arr encryption key and Caddy storage available.
2. Rehearse on a copy using [Database migration](DATABASE_MIGRATION.md).
   Start the new API against the isolated database and deploy the matching
   frontend. Verify local playback and progress first, then check a private
   Source Agent source as its owner and as a separate shared account.
3. Confirm pending migrations are applied and readiness succeeds before
   serving the new frontend. Leave existing agent identities intact.
   Existing agents project previously indexed entries on their next signed
   confirmations in bounded batches.
4. If a Source Agent drops offline, check its heartbeat, publication status,
   and both sets of grants. Discovery can remain visible for up to five
   minutes after confirmation. Playback requires a confirmation and
   heartbeat within 90 seconds. Refresh the catalog after a material file
   replacement. If the agent identity was revoked, follow the re-enrollment
   steps in [Source Agents](SOURCE_AGENT.md).
5. If a deployed upgrade must be rolled back, keep the upgraded database
   for recovery and preserve a fresh backup of post-upgrade writes. Restore
   the pre-upgrade archive to a separate database and pair it with the
   previous API and frontend versions. SQLx's default migration validation
   rejects applied migrations unknown to the binary. Repoint the API only
   after verifying the restored database and reconciling security changes
   made since the snapshot. Follow [Operations](OPERATIONS.md) for IP and
   TLS recovery.

## Remaining limits

Remote video uses browser-native decoding. Music and book failover requires
trusted byte identity, and hashing currently covers local files. Video resume
between different encodes requires a verified edition and similar duration.
The duration check cannot prove that edits align at every timestamp.
Remote titles without verified metadata use the agent filename and fallback
artwork. Personal page offsets still count source-specific history and
bookmark records. CDN discovery, replication and caching remain outside the
scope of this implementation.
