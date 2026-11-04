# For you, media cards, and activity statistics

Restart the API to apply migration `0011_activity_statistics.sql`, then rebuild/reload
the frontend. This extends the existing progress table and API; it does not create
an independent playback or reading tracker. Existing saved positions are retained.

## Dashboard and cards

Continue Watching and Continue Reading have separate horizontal rails, keyboard-
accessible resume buttons, and independently disabled left/right controls. Videos
use landscape artwork; books use portrait covers. Percentages use recorded position
and total only. Durations, episode numbers, titles, chapters/pages, and activity times
are shown when available. Images use existing metadata artwork, not newly generated
frames. No subtitle/audio language counts are invented: those technical streams are
currently inspected on demand in the player, not indexed in the catalog.

Continuation chooses the most recently updated unfinished entry per normalized
collection (including author for books); unknown collections remain per file. A
completed entry can lead to the next available unread episode/volume in the same
source using the existing member ordering. Unstarted next items have no invented
saved position. Missing, completed, and inaccessible entries are excluded. Chapter
zero with a valid locator is a valid continuation point. Explicitly reopened videos
can continue an unfinished rewatch. Catalog/source limits remain applicable.

Continue cards call the existing player/reader directly. Video progress restores its
saved timestamp; books restore the existing chapter/page locator, falling back to
the saved index or the chapter beginning. This change does not invent historical
paragraph offsets that the reader never recorded.

The shared artwork component supplies landscape episode cards, duration badges,
episode identifiers, actual progress, and portrait series/book covers. Series covers
navigate to the existing series detail/member view. Book covers and titles open a
book detail route (`?source=...&details=...`) with an explicit Read action. Explicit
Watch/Read buttons still start playback/reading. Source compact lists and music keep
their existing workflows. The single existing music player remains mounted while
opening details; explicit video/book playback retains its established stop behavior.

Discovery uses accessible catalog items, saves, and recent opens, with a transparent
genre-overlap preference from saves/recent metadata. Music stays on the Music page.
This is not an external recommendation service or an inferred demographic profile.

New card/dashboard styles use semantic aliases for Grimoire's existing palette and
relative spacing/aspect ratios. They add no separate theme or theme selector.

## Statistics and measurement limits

`GET /api/statistics` authenticates the user and filters every progress row by current
source visibility and media permission. Metadata/group genre overlays use the same
authorized enrichment as the catalog. No filesystem scan or new media probe is needed.
Statistics cover at most the 5,000 newest accessible progress records and disclose
truncation. Missing/offline files can retain activity while source permission remains;
revocation removes their contribution from the current response.

Video progress writes may include `played_seconds` (0–60 per write). The existing
media hook samples elapsed playback, excludes seeking, paused intervals and abnormal
jumps, accounts for playback speed, and conservatively ignores long suspension gaps.
This is client-reported viewing telemetry, not server-verified attention. Old seek
positions are never backfilled into elapsed time. Historical watch time therefore
starts as unavailable; the UI explains that only recorded sessions are included.

Book progress writes may include up to 100 `completed_units` indexes (0–4,999 and
less than the book's total) plus a `completed` flag. Advancing a loaded page/chapter,
scrolling past it, or explicitly finishing the last unit records completion. On the
last page, Finish book is available even when there is no next volume. Book chapter
and comic page units are labelled together rather than pretending comic pages are
novel chapters. The progress queue unions coalesced units; PostgreSQL stores distinct
indexes. A failed/damaged page does not count as read. Repeated updates do not add
additional completions. Standard revision, CSRF, rate, and permission checks still
apply; a rejected/stale revision adds no activity.

Episodes watched counts distinct completed episode files (an episode identifier
must be known). Books completed counts files marked complete, independently from
active reading groups. Completion records are not fabricated for historical book
positions. Whole-series completion remains unavailable because a local source is
not a guaranteed complete series inventory. Watch genre rankings use recorded video
time only; reading rankings use completed reading units only. These rankings never
combine audio, watching, and reading into one score.

An authorized user can report progress for their own account; these counters do
not grant access, acquisition privileges, rewards, or other security-sensitive rights.
Already delivered metadata cannot be recalled from a browser by revoking access.
