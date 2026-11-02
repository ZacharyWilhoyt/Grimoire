#!/usr/bin/env bash
# Manual PostgreSQL backup. Uses libpq service/passfile configuration, never a
# credential-bearing database URI in arguments or a shell-sourced .env file.
set -euo pipefail
umask 077
fail() { printf '%s\n' "$*" >&2; exit 1; }
if [[ $# == 1 && $1 == --help ]]; then
  printf 'Usage: PGSERVICE=grimoire %s /private/backup/directory/grimoire.dump\n' "$0"
  exit 0
fi
if [[ $# != 1 ]]; then
  printf 'Usage: PGSERVICE=grimoire %s /private/backup/directory/grimoire.dump\n' "$0" >&2
  exit 2
fi
[[ -n ${PGSERVICE:-} ]] || fail 'Set PGSERVICE to an explicitly configured PostgreSQL service profile.'
for program in pg_dump pg_restore mktemp stat ln; do
  command -v "$program" >/dev/null || fail "Required program not found: $program"
done
parent=$(dirname -- "$1")
[[ -d "$parent" ]] || fail 'The destination directory must already exist.'
parent=$(cd -- "$parent" && pwd -P)
[[ -O "$parent" ]] || fail 'The destination directory must be owned by the current user.'
mode=$(stat -c '%a' -- "$parent")
(( (8#$mode & 8#22) == 0 )) || fail 'The destination directory must not be group/world writable.'
output="$parent/$(basename -- "$1")"
[[ ! -e "$output" && ! -L "$output" ]] || fail 'The destination already exists; backups are never overwritten.'
temporary=$(mktemp --tmpdir="$parent" .grimoire-backup.XXXXXXXXXX)
trap 'rm -f -- "$temporary"' EXIT
trap 'exit 130' INT
trap 'exit 143' TERM
pg_dump --format=custom --no-owner --no-privileges --no-password > "$temporary"
# A parseable archive is necessary, though only a restore drill proves recovery.
pg_restore --list "$temporary" >/dev/null
# Same-directory hard linking publishes the finished file atomically and fails
# if another process created the destination. It never replaces an older backup.
ln -T -- "$temporary" "$output"
printf 'Created PostgreSQL backup: %s\n' "$output"
