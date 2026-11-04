# Dedicated Music library

Restart the API to apply `0010_music_library.sql`, then rebuild/reload the frontend.
The Music sidebar entry opens `/#music`. For you and Discover now show video/books;
connected source browsing and My library continue to expose saved music and metadata
editing through their existing APIs.

## Browsing and playback

Songs displays independently scrolling artist/album panels and a compact six-column
song table. Artist selection filters albums and songs; album selection intersects
with it. Clicking a selected row removes that filter. Clear filters removes both.
The selections survive tab changes and ordinary sidebar navigation in the mounted
workspace. Favorite hearts do not change filters or playback. Albums are scoped by
source, artist, and collection; artists combine equal normalized names across sources.
The existing source catalog bounds (32 sources/5,000 files) still apply and its
truncation/offline notices remain visible.

Search uses the existing top search box. Column headings toggle sorting, and the
sort selector retains title, modified, saved, and source ordering. Clicking a song
or pressing Enter/Space plays the visible available queue using the existing
MusicPlayer. Its native audio seeking/volume controls, shuffle, repeat, and next/
previous controls are retained. The same audio element stays mounted while moving
between Music, Discover, For you, and source browsing. The desktop table occupies
20% of viewport height; the player reserves its measured height below it. Narrow
screens retain horizontal table scrolling and wrap player controls.

A fresh workspace/reload starts idle. No queue, track, position, or play/pause state
is restored. Initial music deep links open Music without autoplay. Explicitly chosen
music starts from zero; the shared progress hook still restores video positions.
Existing persistent progress/history records are retained, as are all preferences
and saved-library data. Opening a book or video retains the established behavior of
stopping music.

Duration uses known audio progress totals or extracted duration metadata; unknown
values display an em dash. Gathering local metadata (enable **Re-read unchanged files** for existing scans)
can populate duration where
FFprobe can identify it within the existing extraction limits. The page does not
probe thousands of audio files automatically.

## Playlists and favorites

This checkout previously had no playlist manager. The Playlists tab now supports
create, rename, delete, playing, and removing songs. Use the plus selector beside a
song to add it to an existing playlist. Existing track hearts reuse the saved-library
API; artist and album hearts are independent private music favorites. These survive
reloads through PostgreSQL, rather than browser playback state.

Playlist entries store opaque file keys. The UI resolves them against the current
permission-filtered catalog. References outside that catalog are retained with an
unavailable count; they never grant access or expose hidden track names. Actual
playback and artwork continue to use existing server authorization. Favorite metadata
and playlist names are account-private. Removing a playlist does not remove its media.

## API and limits

Authenticated `GET /api/music/library` returns `{revision,library:{favorites,playlists}}`.
`PUT` takes `{expected_revision,library}` and requires the normal Origin/CSRF checks.
Every database operation is scoped to the authenticated user ID. A revision mismatch
returns 409 without overwriting the other window's changes; Reload music preferences
fetches the latest saved version.

Storage bounds: a 1 MiB request body, 2,000 unique favorite keys (4,096 bytes each),
100 playlists, 100 characters per playlist name, 1,000 references per playlist,
5,000 references in total. Playlist IDs are UUIDs and references are 64-character
hex file keys. These are preference storage protections, not media acquisition quotas.
Writes are limited to 120 per account per five minutes. No acquisition/sharing policy
changes are introduced.

Tests cover combined filtering, source-scoped albums, independent favorites, tab
state, one audio element across navigation, idle fresh initialization, no music
position restoration, persistence, account isolation, CSRF, invalid references, and
optimistic concurrency. Browser checks use synthetic metadata only.
