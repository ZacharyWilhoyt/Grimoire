# Unified catalog pages

For You, Discover, My Library and Music now consume permitted Source Agent
entries alongside live local scans. The page adapter keeps the original
file keys and adds logical identity, remote availability and a relay URL
when playback is eligible. The frontend groups by logical identity after
applying search and source filters. My Library keeps saved items and also
lists discoverable source media. Existing playlists still resolve their
original file keys. Similar titles alone do not collapse cards.

The latest authorized progress is shown for each logical item. Saved and
progress records remain representation-specific, so opening an alternate
edition or format does not apply a position from the wrong timeline.
Unavailable remote entries remain visible within the existing discovery
window and omit playback links. Revoking discovery hides their catalog,
details and artwork immediately. Bookmarking discovery-only items is
allowed. Opening media and writing progress still require current access.

Remote details use the existing detail and reader layouts. Verified TMDB
episodes use provider series identity and season and episode numbers.
A local TMDB-reviewed series can include permitted remote episodes, while
keeping local sidecar artwork, ordering and group preferences. Remote
music uses the existing player and queue. Remote books use the existing
EPUB/CBZ reader and record progress. Remote video uses browser-native
decoding through the existing authorized range relay. Local video keeps
its transcoding, audio track and subtitle paths.

Migration 0025 adds representation-scoped verified metadata and artwork.
Provider metadata is fetched through the existing verified lookup and
JPEG processing path. Existing verified entries receive their descriptive
fields and artwork on re-verification. Artwork is served by a
permission-checked endpoint. Items without verified metadata use the
agent-supplied name and the existing artwork fallback.

## Verification

The signed agent integration test covers discovery-only browsing, saving,
verified season details and artwork, open history, progress and statistics,
offline visibility, revocation, and actual byte-range relay. Frontend tests
cover deduplicated cards, offline details, source filtering and advancing
across sources without selecting an unavailable episode. The backend tests
use a disposable PostgreSQL database and provider metadata fixtures.

Production was not migrated in this checkpoint. Automatic playback
selection, failover and compatible resume across locations belong to
Phase 5. Personal page offsets still count original saved/history records
for compatibility. Remote-only libraries rely on indexed agent updates
and browser codec support.
