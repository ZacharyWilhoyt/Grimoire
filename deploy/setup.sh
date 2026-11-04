#!/bin/sh
# Prepare new deployment secrets only; never overwrites an existing configuration.
set -eu
umask 077
cd "$(dirname "$0")"
if [ -e .env ] || [ -L .env ]; then
 echo 'deploy/.env already exists; it was left unchanged.'
 exit 0
fi
command -v openssl >/dev/null 2>&1 || { echo 'OpenSSL is required to create deployment secrets.' >&2; exit 1; }
mkdir -p media
media_root="$(pwd)/media"
temporary=$(mktemp .env.setup.XXXXXX)
trap 'rm -f "$temporary"' EXIT HUP INT TERM
admin_password=$(openssl rand -hex 32)
app_password=$(openssl rand -hex 32)
proxy_token=$(openssl rand -hex 32)
integration_key=$(openssl rand -hex 32)
{
 printf 'GRIMOIRE_POSTGRES_ADMIN_PASSWORD=%s\n' "$admin_password"
 printf 'GRIMOIRE_DB_PASSWORD=%s\n' "$app_password"
 printf 'GRIMOIRE_PROXY_TOKEN=%s\n' "$proxy_token"
 printf 'GRIMOIRE_INTEGRATION_KEY=%s\n' "$integration_key"
 printf 'GRIMOIRE_MEDIA_ROOT=%s\n' "$media_root"
} > "$temporary"
chmod 600 "$temporary"
ln "$temporary" .env
printf '%s\n' 'Created private deploy/.env and an empty media directory.' 'Set GRIMOIRE_MEDIA_ROOT to your chosen directory before starting the deployment.'
