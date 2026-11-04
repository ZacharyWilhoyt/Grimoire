# Managed Caddy deployment

This optional Docker Compose deployment adds dedicated Caddy 2.11.8, Grimoire's
Rust API/built frontend, and PostgreSQL. It creates a **new isolated control plane**;
it does not change your current development server, import your database or alter
host services/firewall/DNS. Existing source paths need equivalent container mounts
when migrating an existing database. Back up first; see [Backups](BACKUPS.md).

## First start

Install Docker Engine with Compose v2 and OpenSSL on the host. From the repo:

```sh
./deploy/setup.sh
```

This creates mode-600 `deploy/.env` with separate database/app/proxy/integration
secrets, without overwriting an existing configuration. Edit `GRIMOIRE_MEDIA_ROOT`
to the directory you want mounted read-only at `/media`. Then:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml build
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d postgres
docker compose --env-file deploy/.env -f deploy/compose.yaml run --rm api bootstrap-admin YOUR_USERNAME
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d
```

The bootstrap command uses the existing hidden password prompt. Register source
directories using their **container** path (`/media/...`). Source owners still
choose which encoders viewers can use. Containers default to CPU transcoding;
GPU access requires an explicit device/driver override to the API service. This
setup does not grant GPU access automatically or change existing host playback.

## Before configuring a public domain

Open `https://localhost` on the server. The bootstrap site uses Caddy's private
local CA, not Let's Encrypt. Copy its **public** root certificate out for trust:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml cp caddy:/data/caddy/pki/authorities/local/root.crt ./grimoire-local-root.crt
```

Verify that certificate came from your own deployment, then install it in the
client's trusted CA store. Do not bypass browser certificate warnings or copy
private CA keys. For remote setup, use an SSH tunnel to local port 443, retaining
`localhost` as the HTTPS name. Remove that local trust later if no longer needed.
Unconfigured public hostnames are not served as application sites.

## Public DNS and certificates

Create A records for each configured hostname pointing to this server's public
IPv4 address. Add AAAA records only when the server is actually reachable over
IPv6. Forward/allow inbound **TCP 80 and 443** to Caddy; UDP 443 is optional for
HTTP/3. Caddy needs outbound DNS/HTTPS to Let's Encrypt. Do not configure an extra
CDN/proxy in front without designing its trusted-client-IP handling separately.

In **Administrator security → TLS Certificates**, set the primary domain and
optional additional hostnames, enable Automatic Let's Encrypt management, and
apply. Only these administrator-configured names are requested from Let's Encrypt.
Wildcard/DNS-provider challenges are not implemented. HTTP and TLS-ALPN challenges
use Caddy's normal facilities; ACME validation is handled at the edge without
opening protected application routes. HTTP application requests redirect to HTTPS.

Changing the served names can move the site; sign in again on the new origin once
its certificate is issued. Configure DNS first and keep local recovery access.
Issuance failure can leave a new domain without usable HTTPS even though Caddy
accepted its configuration. Inspect certificate status and private logs:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml logs caddy api
```

Detailed lifecycle logs also persist in Caddy's private `/data/caddy/grimoire.log`
(rotated at 10 MiB, two retained rotations). Do not enable debug/request logging
without considering cookies, private media names and the proxy credential.
The UI returns sanitized errors, public certificate expiry/coverage, and the last
successful renewal **recorded from Caddy events**. It does not invent a renewal
from the certificate's not-before date. No recorded renewal is normal after first
issuance. Observed renewal/error timestamps persist in PostgreSQL.

## Disable automation while keeping HTTPS

Turn off Automatic Let's Encrypt management and apply. Every configured domain
must already have a usable stored certificate/key pair; otherwise the change is
rejected and the old configuration stays active. Grimoire emits
`auto_https disable_certs` and explicit certificate/key paths, preserving HTTPS
termination and redirects. Nothing deletes certificates or private keys.

Caddy 2.11 can remove a former managed certificate from its memory cache when
switching to file-based certificates. The apply sequence reprovisions the manual
configuration once after old-app cleanup so those files are loaded. This uses
Caddy's native reload API, not a process restart. The live regression test checks
HTTPS immediately afterward. Re-enable automation before stored certificates
expire; disabling renewal does not extend their validity.

## Isolation, access control and persistence

Only Caddy publishes ports. The API and PostgreSQL have no host port mappings.
The database has no TCP network at all: the non-superuser app role connects over
a shared Unix socket with SCRAM password authentication. Caddy forwards the
actual socket client IP and replaces the private proxy header. The API accepts it
only from Caddy's pinned private address plus its matching secret and rejects
direct connections. [IP access rules](IP_ACCESS.md) run in Grimoire for frontend,
login, API and media requests, including byte ranges. Caddy's redirects and ACME
validation serve no protected application content.

Caddy's administration API listens **only on a permissioned Unix socket** shared
with the API. It is never a published TCP port or an application route. Generated
configuration has fixed Grimoire upstreams; administrators cannot submit arbitrary
proxy JSON, upstream URLs, file paths or shell commands through the web UI.

Configuration updates adapt/validate through Caddy, then use its atomic `/load`
operation. Database advisory locks serialize changes across API workers; stale
revisions return 409. Caddy rejects bad configurations without replacing the
working one. If database persistence fails after a successful load, Grimoire
attempts rollback; a supervisor reconciles the committed settings every 30 seconds
and after restarts. This is recovery across two systems, not a distributed atomic
transaction. Caddy resumes its own last saved configuration from `/config`.

Named volumes preserve PostgreSQL, Grimoire data, Caddy `/data` (certificates,
private keys, local CA and lifecycle logs), and `/config` (active configuration).
Keep these and `deploy/.env` private and backed up. **Do not use `down -v` on a real
deployment.** API mounts certificate storage read-only; the dedicated proxy owns
issuance. Containers use an unprivileged application identity; the proxy's small
entrypoint initializes volume ownership before dropping privileges.

The example private subnet is `172.30.88.0/24`, Caddy `.2`, API `.3`. If it conflicts
with your network, change the subnet, both service addresses and the trusted proxy
address together. Never broaden trust to all client/contributor networks.

## Recovery

IP lockout, from a local operator terminal:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml exec api grimoire-api reset-ip-access
```

If manual certificates have expired, enable automatic TLS locally and let the
supervisor reconcile (or restart the API):

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml exec api grimoire-api enable-automatic-tls
```

For wrong domains or an inaccessible site, restore local setup mode:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml exec api grimoire-api reset-managed-tls
```

Within 30 seconds the supervisor restores the localhost/internal-CA site. Stored
public certificates/keys remain untouched. Fix DNS/settings, then apply again.
If the Caddy control socket/configuration itself was changed outside Grimoire,
repair permissions/mounts and use its native reload from inside the container:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml exec --user 10001:10001 caddy caddy reload --address unix//run/caddy/admin.sock --config /etc/caddy/bootstrap.Caddyfile --adapter caddyfile
```

Reset the stored TLS settings first so the supervisor does not restore a wrong
domain configuration. Rebuild after dependency updates, preserve volumes, and
recreate both API and Caddy with `up -d --force-recreate api caddy` after rotating `GRIMOIRE_PROXY_TOKEN`. Reconciliation
includes its fingerprint so an old saved header cannot remain active after rotation.

## Checks and limits of verification

Run ignored database tests with the guarded `scripts/test-backend-integration.sh`
launcher and an explicit disposable test `DATABASE_URL`; never rely on dotenv
fallback. The native Caddy tests additionally need `GRIMOIRE_TEST_CADDY_BIN` pointing
to a verified Caddy binary (CI uses `scripts/install-test-caddy.py`).

Backend tests cover strict domain validation, administrator/CSRF boundaries,
revision conflicts, sanitized errors, rejected updates, missing-certificate
refusal, observed renewal timestamps, and committed origin allowlists. A verified
Caddy binary validates both generated modes. `scripts/test-caddy-proxy.py` runs a
loopback-only internal-CA test for HTTPS, actual forwarded-header replacement,
HTTP redirects, byte ranges, WebSockets, automation-to-file transition, rejected
reloads and restart persistence. It never contacts public ACME.

Public Let's Encrypt issuance and elapsed-time renewal need real DNS/ports and
remain deployment checks. Docker/Compose execution requires a Docker host; the
current development environment has no Docker daemon. CI validates Compose, builds the images, and runs a disposable stack smoke test
for HTTPS, login, IP enforcement and persistence. Its result should be checked
before relying on a new image. This deployment does not by itself complete every
public-launch security, abuse-prevention, backup and operational requirement.

Primary references: [Caddy API and atomic reloads](https://caddyserver.com/docs/api),
[automatic HTTPS modes](https://caddyserver.com/docs/caddyfile/options#auto-https),
[explicit TLS certificates and issuers](https://caddyserver.com/docs/caddyfile/directives/tls),
[HTTPS validation requirements](https://caddyserver.com/docs/automatic-https).
