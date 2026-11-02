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
