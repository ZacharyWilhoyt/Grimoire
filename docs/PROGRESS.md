# Personal playback and reading progress

Restart the API to apply migration `0007_media_progress.sql`, then update the web
client. Audio/video resume their saved time when duration/metadata are available.
EPUB resumes a chapter; CBZ resumes a page. Book locations store the archive entry
path as well as its index, so a matching entry takes precedence after reordering.
EPUB scroll offsets/CFI locations and comic page scroll offsets are not yet stored.

Audio/video save while playing at most every ten seconds, and on seeking, pause,
end, hiding/leaving the page, and player disposal. Browser shutdown/network loss
can still prevent a last save; the UI does not claim unsent progress was persisted.
Books save only after the selected chapter/page successfully renders. Native media
controls remain available if progress loading/saving fails; errors are visible.
Seeking before a delayed progress read finishes preserves the user's chosen time.

Progress belongs to the authenticated user, separate from bookmarks/open history.
Each read/write validates the existing source/path and current media permission.
Visibility or acquisition permission alone does not grant progress access. Upload,
metadata edits and group artwork do not change these authorization boundaries.
Disconnecting a source deletes its progress; revocation hides progress without
transferring it to another user. Path-based item identity retains the existing
rename/replacement limitations.

GET `/api/progress?source_id=...&relative_path=...` returns null or a position,
optional total/locator and revision. PUT `/api/progress` accepts the same reference,
position/total/locator and `expected_revision` from that read (zero for a new record).
Audio/video units are seconds; book positions are zero-based chapter/page indexes.
All positions/totals are finite and bounded. Writes require Origin/CSRF checks and
are limited to 120 attempts per account/minute. Advisory transaction locks serialize
first writes as well as updates. Permission writers and progress writers serialize
on the source row. A stale revision returns 409 instead of overwriting another
player's newer state. Clients then stop saving until the item is reopened.

The client serializes writes and coalesces pending positions, so its own overlapping
requests use the last acknowledged revision. Failed writes are not blindly retried
with unknown revisions. Progress is not stored in localStorage, and the browser
never supplies a user ID. Last-opened activity remains distinct from progress:
automatic completion rules are not implemented, and no arbitrary completion
threshold has been invented.

Verification covers position validation, permission/user isolation, CSRF, revision
conflicts, client coalescing/failure handling, and saving the correct media element
when switching tracks. Browser verification is tracked in AUTONOMOUS_WORK.md.

## Catalog and Continue

Catalog/source/library/activity items now include the current user's optional
progress summary (position, total, update time). Cards display saved times or
chapter/page numbers. For you includes a Continue section from `/api/continue`,
independently paginated and sorted by the latest progress save. It does not depend
on a bookmark or an open-history row and does not expose another user's progress.
Only positive positions before the known total qualify; this is not a completion
classification. Books can remain there on their last chapter/page. Unavailable
entries are retained by the API but omitted from the playable Continue section.
The UI shows the first six available entries from the loaded page; refresh or
revisit For you to refresh saved-position labels. Continue uses individual cards
for direct resume; other sections retain the album/series folder presentation.

## Recover history after a rename

For a Compose deployment with bind-mounted media, preview a repair with
`python3 scripts/repair-media-history.py` from the Docker host. Add `--apply`
to move the matched saved items, recent activity, and progress to their current
paths. The script uses the running API's database and matches only a unique
file of the same extension, byte size, and nanosecond modification time within
the original source. The old path must be missing and have an extraction
fingerprint. Unmatched and ambiguous entries are retained for later recovery.
If the new path already has progress, the newer position wins.
