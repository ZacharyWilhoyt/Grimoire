# Copy an existing PostgreSQL installation

This copies the control plane into a fresh `grimoire_import_<random>` database
inside the deployment PostgreSQL container. It never drops/replaces databases,
alters source data, or switches the running API automatically. PostgreSQL
client tools (`psql`, `pg_dump`, `pg_restore`) must support the source server;
the destination must support the dump (this deployment uses PostgreSQL 18).
Run commands from the repository root. The source is selected explicitly from
`backend/.env`, not inherited `DATABASE_URL` or shell-sourced configuration.

## Snapshot and verify

```sh
python3 scripts/migrate-control-plane.py backup --source-env backend/.env --output deploy/runtime/migrations/my-copy
sudo python3 scripts/migrate-control-plane.py restore --backup deploy/runtime/migrations/my-copy
```

Use a new backup directory for each snapshot. `sudo` is needed only if your
account cannot access Docker; it is not needed for the source backup. The
existing deployment PostgreSQL container must already be healthy. Restore uses
its own configured administrator credential internally, never a password in
arguments. Do not paste environment files, dumps or private settings into chat.

The backup holds a read-only repeatable-read exported snapshot while collecting
all public-table row counts and a custom-format dump. SQLx test bookkeeping is
excluded. Restore verifies SHA-256, creates a random new database owned by the
non-superuser `grimoire` role, restores in one transaction, and checks every
public-table row count against that snapshot. A failed restore retains the new
database for inspection; it never deletes another database. Row-count verification
checks completeness, not every application behavior; validate the copy in the UI
before retiring the old installation.

Backups contain private account/session data and, when configured, the original
Arr encryption key. Files are owner-only and deployment-local output is ignored
by Git and Docker builds. Keep independent protected backups per [Backups](BACKUPS.md).
The candidate `deploy/.env.import-<id>` retains deployment passwords/proxy secrets,
selects only the new database, and carries forward the source Arr encryption key
and explicit URL allowlist so existing integration credentials remain decryptable.
Never replace that key with a new random key if encrypted connections exist.
Host-loopback Arr URLs may need owner review/update for container networking;
URLs and their credential bindings are not silently rewritten.

## Review before cutover

The script prints the new database name and candidate environment path and writes
`restore-result.json` with verified counts. `deploy/.env` stays untouched.
The copy includes users, sessions, permissions, source paths, metadata, progress,
playlists and security settings. It does not copy media files, disk cache files,
TLS private keys, or browser-local data. Artwork stored in PostgreSQL is included.
Retain the old installation's data directory and configure the corresponding
persistent files/mounts before cutover. Mount existing media read-only at the
**same absolute paths** recorded in the copied sources; ensure UID 10001 can
traverse/read them. Do not solve permissions with world-writable media.

A snapshot taken while the old API is running is a consistent rehearsal copy,
but later writes remain only in the old database. For a final migration, pause
old application writers, take/restore a fresh snapshot, then use that new
candidate file. Existing container data and the host database remain available
for rollback. Do not run two writable installations against diverging copies.

After reviewing the candidate and mounts, select it with every Compose command:

```sh
# Replace the placeholder with the actual candidate printed by restore.
docker compose --env-file deploy/.env.import-REPLACE_ME -f deploy/compose.yaml up -d --build --no-deps api
```

This recreates only the API; it does not reinitialize PostgreSQL or restart
Caddy. The API applies pending migrations only to the selected imported database.
Use the same candidate environment on subsequent commands, or securely promote
it to your deployment configuration after review. Returning to the prior env
file selects the previous database, without undoing writes made after cutover.
Keep local recovery access: copied IP/TLS policy and source permissions apply.
