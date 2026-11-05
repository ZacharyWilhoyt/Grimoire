# Series and title detail pages

Video and written series share one layout: poster and available metadata in the
sidebar, title/tags/actions/synopsis in the main column, then Episodes or Chapters,
Relations, and Recommendations tabs. Long descriptions expand in place. Missing
backdrop artwork uses the theme's surfaces; posters are never stretched into a
backdrop. Catalog card widths share the Continue Reading cover scale, while
episodes retain landscape artwork.

Open a collection from the catalog to see its series page. Individual books open
the same layout as a title page. Start/Continue and individual entries call the
existing video player or reader with the original saved progress. Sorting,
direction, season and title filtering do not start playback.

Written listings reflect **indexed files** (books/volumes), not an invented index
of every chapter inside an EPUB/CBZ. Internal chapters remain available inside the
reader. Rows retain saved-library controls and metadata editing. Series artwork
and genre controls reuse the owner-only group editor. Unsupported metadata,
ratings, notifications and trailers are omitted. Relations display an empty state
until actual relationship data exists. Recommendations use existing catalog
suggestions restricted to accessible titles with overlapping genres; they are
labelled accordingly.

## Private series preferences

Migration `0012_personal_series.sql` adds a small `user_series` record because no
video/book series intent or favorite model previously existed. Restart the backend
to apply the migration automatically through the existing startup migration flow.

- Status is unset, `planning`, `active`, `completed`, `on_hold`, or `dropped`.
  The UI uses Reading/Watching and Planning to Read/Watch labels.
- Favorite and status belong to the authenticated user and source-scoped group.
  They do not change shared metadata, saved-library bookmarks, file progress,
  completion statistics, or music favorites.
- Explicit status represents the user's intent; setting Completed does not mark
  every episode/page completed or fabricate watch/reading activity.
- Groups derive their identity from source and collection metadata. Changing a
  grouping does not migrate personal intent to an unrelated group. Old preferences
  remain private; source/account deletion removes them.
- Share links retain normal access checks and never grant access.

API: `GET /api/sources/{source}/groups/{group}/personal?path={member}` returns
`{status,favorite,revision}` (revision zero when unset).
`PUT` accepts `{path,status,favorite,expected_revision}` and returns the updated
record. Initial writes use revision zero; later writes must match the current
revision. Conflicts return 409 rather than overwriting another window. Both
routes require media access and a currently matching member; writes additionally
require the existing same-origin/CSRF protections, body limits and rate limits.
Source permission changes are serialized with writes using the existing source
lock convention. Music albums continue using the music-specific preferences.

## Verification

Run `npm test --prefix web`, `npm run build --prefix web`, and backend tests.
The disposable PostgreSQL suite (`cargo test -- --ignored` with a **test**
`DATABASE_URL`) covers private preference isolation, updates/conflicts, CSRF,
revocation, changed group membership and unchanged progress/metadata. Never use a
production database for SQLx integration tests; the test role needs CREATEDB.
