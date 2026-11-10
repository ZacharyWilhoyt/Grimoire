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
