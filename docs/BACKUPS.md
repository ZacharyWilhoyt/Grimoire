# Control-plane backups and restore drills

`scripts/backup-control-plane.sh` is a manual Linux/GNU-tools helper for PostgreSQL
custom-format backups. It does not schedule backups, choose retention, select an
off-site destination, or restore/drop databases. Those remain operator choices.
It uses installed `pg_dump` and `pg_restore`; use a client compatible with the
server version (the recovery drill used PostgreSQL 18 tools and server).

## Connection configuration

The helper requires an explicit `PGSERVICE` profile using PostgreSQL's
[service-file mechanism](https://www.postgresql.org/docs/18/libpq-pgservice.html).
It does not source `backend/.env` or pass `DATABASE_URL` on a command line. Keep
passwords in a private [passfile](https://www.postgresql.org/docs/18/libpq-pgpass.html),
not shell history or process arguments. For example, an operator-managed
`~/.pg_service.conf` can contain:

```ini
[grimoire]
host=127.0.0.1
port=5432
dbname=grimoire
user=grimoire
```

Use `sslmode=verify-full` and the appropriate trusted CA for remote PostgreSQL.
A service profile overrides matching libpq environment variables; inspect the
chosen profile rather than assuming a different PGHOST/PGDATABASE overrides it.
`PGSERVICEFILE` and `PGPASSFILE` can point to explicitly chosen private files.
Passfiles on Unix must have mode 0600. Use a database role with the permissions
needed to read the application tables; do not grant superuser solely for backups.

## Make a backup

Choose an existing private directory outside the repository, owned by the invoking
user and not group/world writable. The helper never creates or overwrites that
directory. Then, from the repository:

```sh
PGSERVICE=grimoire ./scripts/backup-control-plane.sh /private/backups/grimoire-2026-11-02.dump
```

The name is an example; use a new name for each backup. Output has mode 0600. A
failed dump or archive-list validation leaves no published backup. Publication
uses a same-directory hard link, which atomically fails if the destination exists.
An existing file, directory or symlink is never replaced. A successful archive-list
check proves the file is parseable, not that a full recovery will succeed.

The dump includes account/password hashes, sessions, source paths/grants, saved
library/progress, metadata/artwork, and encrypted Arr connection credentials. Treat
it as private account data; `.dump`/`.pgdump` files are ignored by Git as a guard
against accidental commits. Decide encryption, access, off-site storage and
retention before using this for production backups. File mode 0600 is not backup
encryption or a disaster recovery plan.

## Scope outside PostgreSQL

The database dump does **not** include media files in content sources, contributor
hosts, PostgreSQL roles/cluster configuration, or application environment secrets.
Protect `GRIMOIRE_INTEGRATION_KEY` separately: restoring encrypted Arr credentials
without the original key does not recover access to those integrations. Keep an
operator-controlled record of the code/migration version and required secret
configuration. Do not commit these secrets alongside source code.

Source operators retain responsibility for their own media backups. Restored
source paths must resolve to the intended files/mounts; the control plane cannot
recover media that existed only on an unavailable contributor's host.

## Restore into a separate empty database

Create a fresh, disposable database and a distinct service profile such as
`grimoire_restore` that points **only** to that database. Inspect its target first:

```sh
psql --dbname=service=grimoire_restore --no-password -X -Atc 'SELECT current_database();'
pg_restore --dbname=service=grimoire_restore --no-password --single-transaction --exit-on-error --no-owner --no-privileges /private/backups/grimoire-2026-11-02.dump
```

There is deliberately no `--clean` or database-drop command here. The restore is
transactional and should run against an empty target, not the live database.
Recreate intended database roles/privileges separately; restoring without owner/
ACL commands does not provision the production role model. Compare records and
exercise the app against the isolated restore before any cutover.

A restored backup also rolls back security state. Before exposing a recovered
instance, invalidate restored login sessions in that **restored target**:

```sh
psql --dbname=service=grimoire_restore --no-password -X -v ON_ERROR_STOP=1 -c 'DELETE FROM sessions;'
```

Reconcile password changes, disabled accounts, revoked sharing grants and other
security changes made after the backup. Session deletion alone cannot recover
those later changes. Keep the recovered instance private during this review.

## Verification

`python3 -m unittest discover -s scripts/tests` runs safety checks with fake
PostgreSQL executables: private permissions, existing-file/symlink refusal, failed
export/validation cleanup, explicit service configuration and unsafe-directory
rejection. CI runs these checks without a database or credentials.

A real drill against an isolated disposable PostgreSQL cluster exported Grimoire,
restored into a newly created database, compared every public control-plane table's
row count and row-data checksum plus all sequence counters, verified mode 0600 and overwrite refusal, then
removed only the drill database and temporary files. This does not replace an
operator's recovery drill with their real media, secrets and deployment settings.
See PostgreSQL's [SQL-dump documentation](https://www.postgresql.org/docs/18/backup-dump.html)
for the archive format and restore tooling.
