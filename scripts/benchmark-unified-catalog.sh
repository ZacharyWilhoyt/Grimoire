#!/bin/sh
# Start a disposable, socket-only PostgreSQL cluster. No deployment connection is used.
set -eu
: "${GRIMOIRE_TEST_POSTGRES_BIN:?Set this to the directory containing initdb, pg_ctl and psql}"
cd "$(dirname "$0")/.."
bin=$GRIMOIRE_TEST_POSTGRES_BIN
fixture=$(mktemp -d /tmp/grimoire-catalog-bench.XXXXXX)
cleanup() {
    "$bin/pg_ctl" -D "$fixture/data" -m fast stop >/dev/null 2>&1 || true
    rm -rf -- "$fixture"
}
trap cleanup EXIT
trap 'exit 1' HUP INT TERM
"$bin/initdb" -D "$fixture/data" --auth=trust --no-locale >/dev/null
"$bin/pg_ctl" -D "$fixture/data" -l "$fixture/server.log" -o "-h '' -k $fixture" -w start >/dev/null
query() {
    "$bin/psql" -X -h "$fixture" -p 5432 -U "$(id -un)" -d postgres -v ON_ERROR_STOP=1 "$@"
}
for migration in backend/migrations/*.sql; do
    query -q -f "$migration"
done
query -f scripts/catalog-benchmark.sql
