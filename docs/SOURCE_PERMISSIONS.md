# Source permissions

A source owner can configure an explicit grant for an existing username. New users
and users without a grant are denied by default. Creating an account, being an
administrator, or possessing a source ID does not grant access to another owner's
source. Local directory registration still requires an administrator; source grant
management requires ownership and works for future non-administrator source owners
as well. Uploads and contributor nodes remain separate planned workflows.

## Using permissions

1. Select a source you own in the sidebar.
2. Choose **Permissions** beside **Disconnect**.
3. Enter an existing account's username or choose **Edit** on an existing grant.
4. Set source visibility, media access, and the acquisition policy, then save.
5. In the receiving account, use **Refresh sources** in the sidebar or reload.

The three permissions have distinct effects:

| Permission | Effect |
| --- | --- |
| Source visibility | Shows the source name and availability in that user's source list. It exposes no file names or contents by itself. |
| Media access | Allows scanning/browsing, audio/video playback, and EPUB/CBZ reading for this source. Visibility is also required. |
| Acquisition policy | `auto_deny` (default), `manual_approval`, or `auto_accept` for future acquisition requests. Visibility is also required. This never grants media access. |

Media access in this first version is **source-wide**: it applies to all supported
files in the directory and its subfolders, including files added later. The current local-directory model uses the source owner as the access-policy
owner for its files. File-specific media ownership/sharing rules remain future
work. When uploaded or distributed copies have an independent media owner,
delivery must additionally enforce that owner's policy; a source possessing a
file must never be treated as permission to share it. Acquisition adapters and request
processing are not implemented; the policy is stored and displayed but does not
trigger any downloads or approval queues yet. It will be a server-side prerequisite
of the future request workflow, with quotas/validation still to be designed.

Clearing visibility in the editor also clears media access and restores acquisition
to auto-deny. The API rejects inconsistent combinations rather than accepting an
invisible source with active media/acquisition grants. **Remove** deletes that user's
explicit grant and restores default deny. Owners retain intrinsic visibility,
media access, and management; their acquisition policy also defaults to auto-deny.
Owners can enter their own username to explicitly change their acquisition policy,
but cannot revoke their own intrinsic source/media access through this editor.

## API and enforcement

All endpoints require an authenticated session. Mutations additionally require the
configured Origin and the session's CSRF token.

- `GET /api/sources` returns owned sources and explicitly visible shared sources.
  Responses include `is_owner`, `can_access_media`, and `acquisition_policy`, and
  omit internal directory paths.
- `GET /api/sources/{id}/permissions` lists explicit grants; only the owner can read it.
- `PUT /api/sources/{id}/permissions/{username}` creates/replaces one grant. Its JSON
  fields are `can_discover`, `can_access_media`, and `acquisition_policy`. Omitted
  fields default to false/false/auto-deny; unknown fields and policy values fail.
- `DELETE /api/sources/{id}/permissions/{username}` revokes that explicit grant.
- Scans and audio/video/book endpoints require ownership or a grant with **both**
  visibility and media access. Acquisition permission is never used to authorize
  delivery. These checks run on every request, including byte ranges.
- Disconnecting a source remains owner-only; shared users cannot delete it or edit
  grants, and administrators have no implicit override on another owner's source.

Non-owners receive 404 when querying/changing a permission roster. Hidden sources
and sources without media permission return 404 from file endpoints. This avoids
confirming file/source existence to unauthorized callers. A visibility-only user
gets an explicit UI explanation without fetching file metadata.

Permission updates/revocations are transactional and serialized per source. PostgreSQL
constraints enforce default-deny combinations, and `source_permission_audit` records
the actor, target, source ID, old/new states, and timestamp. Disconnecting a source
cascades its grants and audits the revocations, retaining the permission audit trail.
Each new request uses current database permissions, so a revoked user cannot continue
through a stale URL or a future range request. Refreshing the source list also closes
players/readers whose media grant is no longer present. Already delivered content,
buffered media, and a response already in progress cannot be recalled.

## Upgrade and verification

Restart the Rust API to apply migration `0002_source_permissions.sql` automatically.
Existing sources remain private to their owners; migration creates no shared grants.
Restart/update the frontend as usual. No database rebuild or legacy re-import is needed.

`cargo test` checks grant defaults/validation and existing unit boundaries. PostgreSQL
integration checks (`cargo test -- --ignored` with a disposable test role/database)
exercise visibility-only grants, independent acquisition/media settings, owner-only
editing, attempted self-escalation, shared delivery, grant removal, and source
removal. See [authentication setup](AUTHENTICATION.md) for the test database setup.
