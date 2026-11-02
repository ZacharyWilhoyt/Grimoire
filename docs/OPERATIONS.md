# API process operations

The API binds to loopback (`127.0.0.1`, `GRIMOIRE_PORT`, default 3000).
Public hosting still requires the TLS/reverse-proxy, database, secrets and origin
configuration described in AUTHENTICATION.md. This document does not choose a
public deployment hostname or expose the listener.

## Shutdown and restarts

On Unix, SIGTERM and Ctrl+C/SIGINT start graceful shutdown. On other platforms,
Ctrl+C is supported. The listener stops accepting new connections and active
HTTP requests get up to **30 seconds** to finish. After that deadline, remaining
requests are closed so a stalled video/audio stream cannot prevent a restart.
A supervisor should allow more than 30 seconds before sending SIGKILL (for example,
40 seconds). An immediate forced kill cannot provide this drain period.

Media clients may need to reopen/reload after a restart, using their last saved
progress. Graceful draining reduces interrupted requests; it does not guarantee
that a browser has submitted its latest unsaved position. Private media access is
still checked on new requests after reconnecting.

Background metadata jobs are not durable queue workers and are not included in
HTTP draining. They can stop when the process exits; the existing stale-job status
allows an owner to start them again. Do not assume that a shutdown message means
all background indexing work finished.

Shutdown logs contain a drain-start message and, only if needed, a deadline
message. They do not include request credentials, paths or bearer tokens.

## Verification

The shutdown implementation was smoke-tested against the running API and an
isolated disposable PostgreSQL database: an authenticated 32 MiB synthetic media
response completed byte-for-byte after SIGTERM, and a client that stopped reading
was disconnected at the 30-second deadline. Both processes exited with status 0.
Rust unit tests and strict Clippy pass. CI continues to run the existing database
and media extraction checks; these process-signal smoke tests are manual checks.

## Liveness and readiness

- `GET /api/health` is a lightweight process-liveness check, returning HTTP 200
  while the API can handle requests. It deliberately does not query PostgreSQL.
- `GET /api/ready` checks PostgreSQL with `SELECT 1` and returns HTTP 200 with
  `{"status":"ready","service":"grimoire-api"}`, or HTTP 503 with
  `{"status":"not_ready","service":"grimoire-api"}`. It has a two-second
  database deadline, at most two concurrent probes per process, and no-store
  caching. Database error details, connection information and credentials are
  never returned.

Both checks are session-free so a local supervisor can probe the API without an
account. Prefer keeping operational probes internal in a public reverse-proxy
configuration. Readiness does not inspect individual content sources, Arr apps,
metadata providers or codec tools: an offline community source should not make
the entire control plane unready. API startup completes migrations before opening
the listener. During shutdown the listener stops accepting new connections.

Database integration tests verify readiness success, pool saturation/deadline,
recovery, closed-pool failure and independent process liveness.
