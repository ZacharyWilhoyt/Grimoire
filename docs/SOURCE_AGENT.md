# Source Agent

The standalone Rust agent currently supports enrollment and heartbeat. Remote
indexing and playback are pending. Existing server-local sources need no agent.

Build on the agent host with
`cargo build --locked --release --manifest-path backend/Cargo.toml --bin grimoire-source-agent`.
Run it as a dedicated unprivileged user. Set `GRIMOIRE_AGENT_STATE` to a file
in a directory accessible only to that account. The default file is
`grimoire-agent.json` in the working directory. Protect and back it up as a
private signing key.

Create a short-lived token using the signed-in coordinator enrollment API in
[NODE_PROTOCOL.md](NODE_PROTOCOL.md). The enrollment UI is planned for the
operations phase. On the agent host run:

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
Remote streaming and the frontend source flow are pending.
