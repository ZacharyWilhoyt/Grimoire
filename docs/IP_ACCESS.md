# Administrator IP access

Administrator security → IP access control configures instance-wide IPv4/IPv6
addresses or CIDRs. Allow matches override Deny matches; the explicit default
applies otherwise. Initial policy is Allow with empty lists, preserving existing
behavior. Review rules before saving. A policy excluding the current administrator
requires explicit lockout acknowledgement. Revision conflicts require reloading.

Rules cover new frontend, login, API and media requests. They do not terminate
bytes already delivered or an existing stream. Policies live in PostgreSQL and
are read on every request, so changes apply across workers immediately. Database
failure fails closed. In direct local development (require-proxy disabled), unproxied loopback `/api/health` and `/api/ready`
probes remain available without serving application content during DB outages.
There is no privileged browser/network bypass for application resources.

Local recovery (requires operator access and database credentials):

```sh
cargo run --manifest-path backend/Cargo.toml -- reset-ip-access
```

This restores default Allow and empty IP lists. Accounts, sessions and source
permissions remain enforced. Both administrative changes and recovery create
an auth audit event. Keep a local terminal available before restricting access.

Forwarding headers are ignored for untrusted peers. For a dedicated reverse
proxy, configure both `GRIMOIRE_TRUSTED_PROXIES` (JSON array of exact proxy IPs or
CIDRs) and a private `GRIMOIRE_PROXY_TOKEN` (64 hexadecimal characters).
The proxy must replace `X-Grimoire-Client-IP` with the actual socket client IP
and `X-Grimoire-Proxy-Token` with that token. A trusted peer missing either valid
header is rejected. `GRIMOIRE_REQUIRE_PROXY=true` also rejects direct connections
from other peers. Do not use broad trusted networks for community/client hosts.
The authenticated client IP also supplies login/registration rate-limit keys.
Arbitrary X-Forwarded-For and Forwarded headers never determine policy identity.

API: admin-only `GET/PUT /api/admin/security/ip` and
`POST /api/admin/security/ip/preview`. Mutations retain Origin/CSRF enforcement,
body limits, canonical IP validation and bounded lists; updates are rate limited.
No proxy secret is returned to the browser.
