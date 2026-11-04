#!/bin/sh
set -eu
umask 077
mkdir -p /data/caddy /config/caddy /run/caddy
chown 10001:10001 /data/caddy /config/caddy /run/caddy
exec su-exec 10001:10001 sh -c 'chmod 700 /data/caddy /config/caddy /run/caddy; exec caddy run --resume --config /etc/caddy/bootstrap.Caddyfile --adapter caddyfile'
