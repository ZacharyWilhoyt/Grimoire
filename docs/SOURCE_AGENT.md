# Source Agent

The standalone Rust agent supports enrollment, catalog sync and outbound
relay playback. Existing server-local sources need no agent.

Build on the agent host with
`cargo build --locked --release --manifest-path backend/Cargo.toml --bin grimoire-source-agent`.
Run it as a dedicated unprivileged user. Set `GRIMOIRE_AGENT_STATE` to a file
in a directory accessible only to that account. The default file is
`grimoire-agent.json` in the working directory. Protect and back it up as a
private signing key.

Create a short-lived token from **Source Agents** in the coordinator sidebar.
On the agent host run:

```sh
grimoire-source-agent enroll https://grimoire.example.com "Living room"
grimoire-source-agent run
```

The enrollment command prompts for the token. The coordinator must be reachable
over HTTPS with a trusted certificate. Allow the agent's outbound IP in the
instance's access policy when applicable. The heartbeat only requires outbound
HTTPS. A revoked agent cannot renew its status. Preserve its private state file
across restarts.

Stop the agent before running `grimoire-source-agent rotate`. The command
keeps a private pending key next to the current identity until the coordinator
accepts it. If the connection drops, rerun the rotation command with both
files intact. Restart the agent after rotation succeeds.

## Approved media roots

Set `GRIMOIRE_AGENT_CONFIG` to a private JSON file on the agent host. Each
entry needs a UUID you generate once and keep, a display name, and an
absolute canonical root. The agent skips symlinks and reads file metadata.
A source is unpublished by default. Publishing without viewer grants keeps
it owner-only.

```json
{
  "sources": [
    {
      "id": "72b79051-778c-4429-8943-2b55f024cbec",
      "name": "Living room media",
      "root": "/srv/media",
      "published": true,
      "grants": []
    }
  ]
}
```

The agent keeps its private revision and item IDs in a catalog directory next
to its identity file. Back up both. Catalog updates resume after a failed
request and replay the pending revision before scanning again. A failed or
incomplete filesystem scan does not publish tombstones. A disconnected source
or one removed from the local configuration expires from the remote catalog.

Viewer grants currently apply at source level. To share metadata, the
agent's grant and the coordinator's source permission must both allow discovery.
The Source Agents panel lists remote catalogs separately from Discover.

## Outbound playback preview

Use **Source Agents** in the coordinator sidebar to generate an enrollment
token, view status, revoke your agent or browse remote sources. A source
owner can grant central access using **Permissions**. The agent's local
configuration must grant the same user access. Copy the UUID shown in the
central permissions roster into the source’s local `grants` array. Apply local configuration
changes with an atomic file rename. The agent rereads the configuration
before every file range, so unpublishing takes effect for new reads even
before catalog sync.

Relay file access currently requires Linux with `openat2` support. It opens
the indexed file beneath an approved root without following symlinks. Remote
audio and video use browser-compatible originals. EPUB and CBZ use the
existing range-based reader. Remote saved progress, subtitles, transcoding,
direct delivery and per-directory or item sharing rules remain pending.

For private HTTPS, set `GRIMOIRE_AGENT_CA_FILE` to the trusted coordinator
CA certificate. Verify the certificate through a trusted operator channel
before installing it. Keep the identity, pending key during rotation, and
catalog journal private.

### Separate Compose deployment

Create `deploy/agent-config/sources.json` on the agent host and use `/media`
as the approved root inside the container. Make the configuration readable
by UID 10001. Set `GRIMOIRE_AGENT_MEDIA_ROOT` to the directory on the host
and grant that UID read and directory traversal access. Run:

```sh
docker compose -f deploy/compose.source-agent.yaml build
docker compose -f deploy/compose.source-agent.yaml run --rm agent enroll https://grimoire.example.com "My agent"
docker compose -f deploy/compose.source-agent.yaml up -d
```

This service has no published port or Docker socket. Back up its private
state volume and configuration before moving or reinstalling it.

### Relay smoke test

After building the debug binaries, run
`scripts/test-source-agent-relay.py` with Python and `pexpect` installed.
Set `GRIMOIRE_TEST_CADDY_BIN` to a verified Caddy binary and
`GRIMOIRE_TEST_POSTGRES_BIN` to the PostgreSQL server bin directory.
The test starts its own loopback-only coordinator, CA, proxy, agent and
temporary database. It does not use the running Grimoire database.
