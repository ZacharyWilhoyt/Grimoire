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
completion percentages/automatic completion and a dedicated Continue section are
not yet implemented, and no arbitrary completion threshold has been invented.

Verification covers position validation, permission/user isolation, CSRF, revision
conflicts, client coalescing/failure handling, and saving the correct media element
when switching tracks. Browser verification is tracked in AUTONOMOUS_WORK.md.
