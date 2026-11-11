# Unified catalog matching and review

Migration 0024 adds verified identity groups, append-only decisions, curator
cases, permission-filtered projections, and on-demand local content hashing.
It preserves the source agent protocol, original locations, legacy media
identifiers, and existing playback and progress APIs.

An administrator can verify a provider identity through the existing HTTPS
provider adapters. The curator confirms which specific file and edition or cut
the evidence applies to. Identical provider, entity, identifier and edition
keys group automatically. TMDB episode keys use the series, season and episode
together. MusicBrainz uses recordings. Open Library uses works and requires an
explicit edition discriminator. Conflicts are recorded and stay separate
until a curator splits and verifies the correction. The catalog still
derives its displayed filename and playback eligibility from a viewer-visible
location.

Weak basename or eligible full-hash matches appear on demand for source
owners. They never establish logical classification. A decline suppresses
the same pair and names until the candidate changes. Owners can select their
own files by directory, series and optional season, or album and preview a
batch of up to 50 changes. Files can be omitted before applying. A checked
preview protects against changes between review and submission. Link, unlink
and split decisions can be undone while still current. Original IDs continue
to resolve while the viewer can discover the corresponding location. The
server retains original representations and does not rewrite legacy progress.

Explicit reports are separate from suggestions. Reports are deduplicated per
representation, rate-limited per requester, and subject to open case caps.
Administrators can resolve cases and suppress noisy sources. Case review still
requires source discovery access.

Local owners can request a full SHA-256 on demand and poll its status. The
worker streams the file in 1 MiB chunks with pacing, a 15-minute timeout and
a 64 GiB size cap. Size and modification time protect the association against
normal local changes. Source Agent hashes are not computed over the relay or
accepted as verified claims. Remote copies can use provider verification or
owner review.

## API

All routes require a signed-in session. Writes use the usual CSRF protection.

| Method | Route | Purpose |
| --- | --- | --- |
| GET | /api/catalog/v1/items | Logical items with a UUID cursor |
| GET | /api/catalog/v1/items/{id} | Resolve a visible item or original ID |
| GET | /api/catalog/v1/locations/{location}/suggestions | Owner-only weak matches |
| GET | /api/catalog/v1/sources/{source}/review-candidates | Select own files with pattern=directory, series or album and value, optional season and after |
| POST | /api/catalog/v1/review/preview | Validate explicit changes and return expected state |
| POST | /api/catalog/v1/review | Apply the selected changes with their expected state |
| POST | /api/catalog/v1/review/{batch}/undo | Reverse a current batch with a reason |
| GET, POST | /api/catalog/v1/locations/{location}/hash | Poll or start a local owner hash |
| POST | /api/catalog/v1/locations/{location}/report | Report a visible representation with a reason |
| POST | /api/admin/catalog/v1/verify | Verify provider evidence for a representation and explicit edition |
| GET | /api/admin/catalog/v1/cases | Paginate current visible cases |
| POST | /api/admin/catalog/v1/cases/{id}/resolve | Resolve a visible case with a reason |
| POST | /api/admin/catalog/v1/sources/{source}/suppression | Change suggestion and report suppression with a reason |

A review body takes a reason and an array of changes. Each change includes
a location_id, an action, an optional target location ID, and the expected
object returned by the preview when submitting. Link and decline require
a target. Unlink and split do not. Administrator reviews opt into curator
scope explicitly.

## Deployment and verification

Existing pages keep using their current endpoints until Phase 4. The new
listing reports eligibility from the index. Playback still revalidates the
location at request time. Production was not migrated in this checkpoint.
Back up the control-plane database before upgrading. The earlier coordinator
can ignore these additive tables if rolled back. Retain them to preserve
decisions and evidence.

Tests use a disposable PostgreSQL database. The cross-source fixture exercises
local and remote projection, entity and edition separation, conflicts, owner
and curator permissions, private candidates, pagination, decline replay,
batch exceptions, undo, reports and suppression. It uses deterministic
provider evidence at the storage boundary rather than contacting external
services. The hash fixture reads an actual file and checks the digest and
invalidation after an inventory change.
