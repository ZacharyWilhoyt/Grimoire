# For you, Discover, and My library

The three navigation pages use actual source files and PostgreSQL-backed personal
state. The sample catalog and mock playback progress have been removed. Links use
`#for-you`, `#discover`, and `#library`, with browser back/forward and reload support.

- **For you** shows a private Continue section from saved progress, recently opened items, saved items, and a rotating selection by
  media type from accessible sources. Suggestions balance video, music, and books;
  they do not claim to infer tastes or use external metadata/recommendation services.
- **Discover** browses supported files across sources for which the account currently
  has both visibility and media access (or ownership). Search matches filenames,
  relative paths, and source names. Media-type/source filters and title, source,
  or modification-date sorting operate on the loaded catalog.
- **My library** contains items the user explicitly bookmarks from Discover, For you,
  or a source. Saves persist across logout and API restarts. Removing a bookmark
  changes only personal state and never deletes media or changes permissions.
  Library pages load 100 saved records at a time, with **Load more** for additional
  records. Search/sort/filter apply to the loaded items.

All pages can open the existing audio/video players and EPUB/CBZ reader. The music
queue uses the visible music items, supports tracks from different sources, and
records activity when advancing. The Ctrl/Cmd+K shortcut focuses global search.

## Personal state and identity

An item key is SHA-256 of source ID, a separator, and its relative path. Identical
filenames in different sources remain distinct. Renames create a different item;
replacing a file at the same path retains its personal state. This is a path-based
identity, not a content hash or metadata-provider identifier.

`user_media` stores only the authenticated account ID, item/source references,
save timestamp, and last-opened timestamp. It stores no duplicated titles, embedded
metadata, or origin URLs. Activity means **opened**, not watched/listened/read or
completed. Playback and chapter/page positions now persist through the separate [progress API](PROGRESS.md).
Completion, playlists and taste-based recommendation ranking remain unimplemented.

A saved record is not an access grant. Every catalog/library/activity query and
mutation checks current source/media permission. Visibility-only or acquisition-only
grants never expose file metadata. Revoked sources disappear from all three pages;
personal records remain private and can reappear if permission is restored. Sources
that are disconnected cascade their personal records. Unavailable files in otherwise
authorized sources remain visible in the saved/recent lists as **Unavailable**;
opening is disabled, but the bookmark can still be removed. Files and source grants
are never modified by library actions.

## APIs and scan limits

These session-protected routes require Origin and CSRF tokens for mutations:

- `GET /api/catalog`: bounded live scan with personal-state enrichment. Returns
  `items`, `truncated`, and a count of `unavailable_sources`. No absolute paths
  are exposed. Metadata is filtered again against current grants after filesystem work.
- `GET /api/library?offset=0`: paginated saved items, with `has_more`, `next_offset`,
  and `unavailable_items`.
- `PUT /api/library`: save `{ "source_id": "...", "relative_path": "..." }` after
  validating access, file type, path containment, and existence. Repeat saves are
  idempotent and preserve the original save date.
- `DELETE /api/library/{file_key}`: remove only the current account's save. This can
  clear personal state even if source access has been revoked; it reveals no item data.
- `GET /api/activity?offset=0`: recent-open history with the same pagination shape.
- `POST /api/activity/open`: validate an accessible file reference and update its
  last-opened timestamp. No user ID, title, or source path is accepted from the client.

The first catalog implementation scans live rather than introducing a background
indexer: up to 32 authorized sources and 5,000 files in a preview. It inherits the
existing per-source limits of 10,000 media files, 100,000 directory entries, and 32
subdirectory levels. Browse a source directly for files outside the global preview.
Two filesystem scan/stat jobs may run concurrently; saturation returns 429 and the
UI provides Refresh. These are operational response/work bounds, not storage quotas.
An indexed catalog with incremental source refresh is future scalability work.

Media-reference mutations permit a bounded 16 KiB JSON body to accommodate long
relative paths; account requests keep their 4 KiB limit. Symlinks, traversal, and
path aliases are rejected when recording personal references. Requests are
parameterized and keyed to the authenticated user. New stream/range requests still
recheck source permissions independently of the personal library.

Restart the Rust API to apply migration `0003_personal_media.sql`, then restart the
frontend. Existing users and sources are preserved; no database rebuild or re-import
is needed. The frontend build, selector tests, existing media tests, and PostgreSQL
integration tests cover access isolation, revocation, save/unsave, history, missing
files, offline sources, and library pagination.

## Music queue controls

The player now offers Shuffle and Repeat (off → all → one). Both default to off
and last for the open player only. Shuffle keeps a stable permutation of the
selected queue, starting at the current track and visiting each entry once before
repeat-all wraps. Previous follows that same order. Repeat-one uses native audio
looping; repeat-all wraps the queue, including a single-track queue. These controls
never expand the queue beyond the accessible items selected by the current view or
folder. Saved progress and current delivery permission checks still apply.

EPUB chapter selectors use titles from the EPUB3 navigation TOC or EPUB2 NCX when
available, with numbered labels as fallback. The package spine remains the
reading order. Fragment links into the same spine file do not create separate
reader chapters; the first matching TOC title labels that spine entry.

For you loads its independent sections even when one endpoint fails. The error
notice identifies the unavailable section; healthy sections remain usable.
Refresh retries the page. Requests remain sequential to respect scan limits,
and navigating away cancels the remaining load.

The EPUB reader offers Paper (default) and Night themes. These and text sizing
apply only to the open reader session and do not persist across account sessions.
Changing text size or theme redraws the current chapter and can return its view to
the top; saved progress remains at the same chapter. The iframe sandbox and
external-resource restrictions are unchanged.

Playback errors offer Retry audio / Retry video. Retry reconnects to the protected
stream, reloads saved progress and preserves volume, mute and playback speed.
It does not convert unsupported codecs or bypass a revoked permission. The current
music queue and selected video subtitle track remain selected during retry.
