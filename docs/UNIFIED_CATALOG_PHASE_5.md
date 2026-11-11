# Playback routing and failover

The unified video, music and reader paths resolve their initial source through
the authenticated playback API. The API takes a catalog reference and the
browser's supported container hints. It only considers currently playable
locations. Users can choose another eligible copy from the player's source
selector. Overrides last for the current playback session.

The resolver prefers the requested encode when alternate quality is
unknown. For equivalent encodes it favors a local path and then a healthy
relay. It learns remote delivery cost from actual successful range chunks
and timeouts, with a two-minute lifetime and a smoothed estimate per node.
Selection does not probe agents in the background. Browser container hints
are advisory since the container alone cannot establish codec support.

Source choices are scoped to the signed-in session, kept in coordinator
memory, and bounded by expiry, capacity, rate limits and candidate limits.
Each switch rechecks visibility, media permission, liveness and
compatibility. The stream, transcode and book endpoints continue enforcing
their own permissions. The resolver does not issue media credentials or
bypass an agent's policy. There is no new database migration.

## Continuity

A network or media error can select another compatible location, up to
three automatic switches. A prolonged relay stall also triggers a check.
An interrupted book switches only to a byte-identical archive of the same
format. Music copies also require byte-identical evidence. A video can
switch between encodes of the same curator-verified edition or cut.
The player restores the last position across encodes only if the browser
reports a duration within one second of the prior duration. Otherwise it
uses the target's own saved position. Existing progress records stay tied
to their original representations.

The resolver uses verified hashes where available. The current hashing
worker only verifies local files, so cross-node book and music failover
requires future trusted evidence. A remote video still needs native codec
support. Local video retains its existing transcoding, audio track and
subtitle controls. Source Agent management previews and unindexed local
files can still use their existing explicit playback paths.

## Verification

The PostgreSQL integration test covers initial selection, manual override,
session isolation, exclusion of unrelated and private copies, revocation,
and stopping when no compatible source remains. Frontend tests check
seeking after a compatible switch and refusing a seek across mismatched
durations. The live HTTPS smoke test uses an actual Source Agent for
range playback and switches to a local encode after the agent is revoked.

Production was not redeployed in this checkpoint.
