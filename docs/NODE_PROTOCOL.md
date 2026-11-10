# Source Agent protocol v1

The coordinator terminates HTTPS and applies its instance IP policy before
accepting node requests. Node requests do not use browser cookies. Owner-facing
endpoints use the usual authenticated session, origin and CSRF checks.

## Enrollment and liveness

| Method and path | Authentication | Result |
|---|---|---|
| POST /api/nodes/v1/enrollment-tokens | Owner session and CSRF | 201 with `token` and `expires_in` |
| POST /api/nodes/v1/enroll | Single-use token and Ed25519 proof | 201 |
| POST /api/nodes/v1/heartbeat | Enrolled Ed25519 key | 204 |
| POST /api/nodes/v1/rotate | Current and replacement Ed25519 keys | 204 |
| GET /api/nodes/v1 | Owner session | Owner's enrolled nodes and status |
| DELETE /api/nodes/v1/{id} | Owner session and CSRF | 204 and immediate revocation |

The request body is JSON, capped at 4 KiB. Enrollment accepts `version` (1),
`node_id` (non-nil UUID), `public_key` (32-byte Ed25519 key as hex), `token`
(32 random bytes as hex), `label` (1–80 non-control characters), and
`signature` (64 bytes as hex). Sign the following UTF-8 lines joined with LF
and no final newline:

```text
grimoire-node/enroll/v1
<node_id in canonical UUID form>
<public_key as transmitted>
<token as transmitted>
<label as transmitted>
```

The coordinator stores only the token hash and binds the key to the owner who
issued the token. Tokens expire after ten minutes and are consumed in the same
transaction that binds the key. Node IDs and public keys are unique. The
enrollment proof pins the key and subsequent requests use that key.

Heartbeat JSON contains `version`, `node_id`, `timestamp_ms` (integer Unix
milliseconds), and `signature`. Sign these UTF-8 lines:

```text
grimoire-node/heartbeat/v1
<node_id in canonical UUID form>
<timestamp_ms as a decimal integer>
```

A heartbeat must be within 60 seconds of the coordinator clock and newer than
the last accepted heartbeat for that node. The agent sends one every 30 seconds
and the coordinator reports it offline after 90 seconds without contact.
Keep both machines' clocks synchronized. Protocol mismatch returns 426;
invalid, replayed, expired or revoked credentials return 401. Rate limits
return 429. The enrollment token and private key are excluded from logs.

Rotation accepts `version`, `node_id`, `timestamp_ms`, `new_public_key`,
`signature` and `new_signature`. Both keys sign the same UTF-8 lines:

```text
grimoire-node/rotate/v1
<node_id in canonical UUID form>
<timestamp_ms as a decimal integer>
<new_public_key as transmitted>
```

Rotation uses the heartbeat freshness rules and atomically replaces the key
under the same node ID. The previous key immediately stops authenticating.
Revocation permanently disables the node ID. Catalog, policy and transport
operations will extend this document as they are implemented.

## Catalog revisions

`POST /api/nodes/v1/catalog/delta` uses the headers
`X-Grimoire-Node-Version`, `X-Grimoire-Node-Id`,
`X-Grimoire-Node-Timestamp` (Unix milliseconds) and
`X-Grimoire-Node-Signature` (hex Ed25519 signature). Sign these lines:

```text
grimoire-node/catalog-delta/v1
<node_id in canonical UUID form>
<timestamp_ms as a decimal integer>
<lowercase SHA-256 hex of the exact request body>
```

The body is bounded to 256 KiB. It contains a `source_id` UUID, `source_name`,
positive `revision`, `published` flag, `upserts` and `removed` arrays, and
optional `grants`. Upserts contain opaque `item_id` and `representation_id`
UUIDs, a basename, a supported extension, and the byte length. Removed item
IDs are tombstones. A delta can include up to 256 combined changes. An
included grants array replaces the source-level projection. Omitted grants
preserve it. Grants identify registered users by UUID and distinguish discovery
and media access. New sources and changed indexes can be staged unpublished
until the final batch.

An accepted delta returns 204. The first revision is 1. Identical retries of
the most recent revision are acknowledged without reapplying it. Skipped
revisions or different bytes for an accepted revision return 409. A node can
only update sources bound to its enrolled identity. The catalog is hidden
after 90 seconds without a successful confirmation or heartbeat.

`GET /api/nodes/v1/sources/{id}/catalog` uses a browser session and an optional
`after` item UUID. It returns up to 100 items. For viewers, both coordinator
and agent discovery policies must allow listing. Media eligibility is
reported separately and requires both access grants. Remote catalogs are
still separate from the existing Discover and source views.

## Outbound range relay

The authenticated browser uses
`GET /api/nodes/v1/sources/{source_id}/representations/{representation_id}`.
The coordinator supports single byte ranges, suffix ranges and HEAD. It
checks the browser session, both sharing policies, and source liveness.
The agent keeps an outbound HTTPS poll open for up to eight seconds at
`POST /api/nodes/v1/relay/poll`. No listening port is required at the agent.

Poll and upload use the signed node headers from catalog sync. Sign the
lines `grimoire-node/relay-<operation>/v1`, node UUID, timestamp milliseconds,
and payload, joined by LF. For poll the payload is empty. For
`POST /api/nodes/v1/relay/upload/{job_id}` it is
`<job_id>:<lowercase SHA-256 of the exact body>`. Send that hash in
`X-Grimoire-Content-Digest` as well. An empty upload declines a range.
Only the node named in the pending job may complete it, and a job is
consumed at most once.

A successful poll returns a signed grant. The signature is made with the
coordinator's ephemeral Ed25519 key and the key is delivered over the same
trusted HTTPS connection. The signed payload binds the job, node, source,
representation, viewer, owner flag, offset, length, file size and expiration.
Grants last 15 seconds and allow at most 2 MiB per job. The agent reopens
the current local policy and checks the file identity before each range.
The coordinator rechecks the session and permissions before releasing bytes.

The relay carries media bytes through the coordinator. It limits concurrent
streams to 16 globally and four per node, paces each stream, and applies
separate poll and upload rate limits. Instance administrators can read
process-local counters at `GET /api/admin/relay`. Operational tuning beyond
these fixed limits is pending. The coordinator does not fetch agent-supplied
URLs or run FFmpeg against relayed originals. Direct HTTPS delivery is a
separate pending transport.
