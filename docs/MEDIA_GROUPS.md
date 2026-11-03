# Album/series folders, covers, tags, and credits

After updating, restart the API (migration `0006_group_metadata.sql` runs at startup)
and restart Vite or rebuild/deploy the frontend. No additional environment settings
are required. Local extraction and optional reviewed online/Arr matching remain
available.

## Browse folders

**Album / series** is now the default grouping view. A collection occupies one
folder card instead of displaying every file at once. Open it to browse its members:

- TV series → season folders → episodes. A series cover/tag edit applies across
  every season. Filename hints can identify a series before local extraction.
- Music albums → tracks, with disc folders when at least two distinct disc numbers
  are known. Unknown disc numbers alone do not create artificial multidisc albums.
- Book series → the available volumes/files in series-position order.

The existing collection field drives grouping for movies too when a movie collection
is present. Files without collection metadata remain individual cards; the app does
not infer unrelated files belong together. Gather embedded metadata, apply a reviewed
match or edit collection/creator fields to organize them. **No grouping** restores a
flat view. **Artist / author / creator** provides a creator folder followed by albums
or series. Breadcrumbs and Back return to parent folders. Music playback within an
opened album/disc uses that folder's ordered tracks.

Folders are catalog views, not filesystem directories. No file is moved, renamed or
copied, and saved references stay unchanged. Folder counts and contents reflect files
loaded and matching the current view/filters. My library folders contain loaded saves;
For you sections contain their recent/saved/suggested subsets. Existing catalog and
pagination limits still apply. Folder labels sort alphabetically for Title; other sort
choices and recent sections retain the input order. Members use season/disc and then
track/episode/series-position ordering.

## Add items to a group

Source owners can drag an individual item onto an album/series folder. While dragging
inside a folder, a destination shelf exposes other compatible groups in the loaded
view. Each eligible item also offers **Move to group** for keyboard and touch use.
Destinations must be in the same source and have the same media kind; season, disc,
and creator folders are not destinations.

Joining copies the destination representative's collection name, creator for music
and books, individual genre tags, and release year when available. The item immediately
inherits the destination group's custom cover and group tag overrides. Its title,
episode/track/disc numbering, description, and individual artwork remain intact.
The underlying file is never moved or renamed. These are custom metadata edits, so
local rescans preserve them. Edit individual metadata or assign another group to
change membership again.

Only available, editable items and groups in the loaded view are offered. The server
independently checks source ownership, media compatibility, and the destination's
current group identity. A destination that changed since loading returns a conflict
instead of silently assigning an outdated group. Membership changes are audited.

## Edit the group

Source owners can use the folder's **Edit group** settings button, or **Edit album /
series** inside a folder. These settings apply to **all current and future members**
in that source, including files outside the current page/filter:

- **Upload poster / album cover:** choose a JPEG, PNG, WebP or GIF in the frontend.
  The server validates the actual image bytes, resizes it to at most 600 pixels per
  side and stores a JPEG in the private artwork cache. The folder, subfolders, and
  every member use the custom cover. **Use individual artwork** removes the group
  override and restores each item's chosen local/provider artwork.
- **Group genres / tags:** one tag per line, up to 20 tags of 60 characters each.
  Saving replaces the displayed tags for every member; an empty list clears them.
  Tags use the existing whitespace normalization, lowercase and deduplication.
  **Use individual tags** restores each item's own tags.

Group settings overlay the complete individual metadata version. They do not rewrite
individual tags, titles, filenames, original covers, or provider records. Local rescans,
reviewed matches and Arr imports preserve group settings. While group tags are active,
the individual metadata form disables its tag control and preserves underlying tags
when saving other fields. Group covers also take precedence over an item's artwork
preference until the group override is reset.

Identity is source-scoped: source ID + media kind + normalized collection, plus creator
for music/books. TV seasons do not split the series identity. Identical labels on
another person's source do not share settings, and the source label distinguishes
folder cards. Case/whitespace-only changes retain the group. Changing collection or
creator to a different identity moves a file into another group; the old group's
settings remain for its other/future members. This is still text-based organization,
not a canonical work/release/edition database. Same-named collections from one creator
within one source can be distinguished using different collection labels.

## Permissions and upload bounds

Only the source owner can change group covers or tags. Visibility, media access, and
acquisition permissions do not grant edit rights; administrators receive no implicit
edit access to another owner's source. Viewers need current visibility **and** media
access to read group details/artwork. Every image request validates a current member
file and its current group, and returns JPEG with `private, no-store` and `nosniff`.
Moving a file out of a group invalidates its old group-image URL. Revocation blocks
future reads; already delivered images cannot be recalled.

Uploads authenticate and check ownership before reading the body. The API limits
images to 8 MiB, 4096 pixels per side and the existing 64 MiB decoder allocation
limit; stored JPEGs are at most 512 KiB. SVG and invalid image data are rejected.
At most two shared metadata/image operations run per API process. Image uploads
are limited to 30 attempts per account per five minutes and have a 30-second body
read deadline. These limits do not replace keeping image libraries maintained or
future isolation for arbitrary media-file uploads. This endpoint uploads only a
cover image, not a content-source media file.

Writes are audited and recheck group identity under the existing source-owner
transaction lock, preventing stale dialogs from changing a newly regrouped file.
Source disconnection cascades group settings. Cached images use existing hash
deduplication; unreferenced artwork and old audit history follow the existing
operator-managed retention limitations. Resetting a cover does not delete media.

## Accurate item credits

The item metadata dialog now credits the providers whose fields or selected artwork
are active for that item. A MusicBrainz/Cover Art Archive item does not get a TMDB
credit, and local/custom-only metadata has no unrelated provider credits. Group tag
and cover overrides are considered when determining whether provider data is still
used. Custom individual fields suppress credits for replaced values. Imported field
values retained unchanged keep their credit (editing one genre does not strip a
retained provider title of attribution), and retained provider artwork also keeps
its credit. Search results/review previews
also credit their provider while that provider's results are displayed.

Arr imports credit the connected Arr app; the current adapter does not assert an
unknown upstream provider. Uploading a group image does not remove a provider credit
if that provider still supplies active title/description/etc. Conversely, overriding
all its remaining fields/artwork removes its item credit.

**Application credits** is a separate general section for Grimoire's integrations.
It retains TMDB's approved logo and required application notice. TMDB requires
attribution in an About/Credits section; see its
[official attribution requirements](https://developer.themoviedb.org/docs/faq).
This general notice does not claim TMDB supplied a music item's metadata.

## API

All endpoints require authentication; mutations also enforce existing Origin/CSRF
checks and source ownership. `{group}` is returned in an item's `metadata.group.id`.
A representative **current member** path is required, preventing arbitrary group-ID
access. Catalog metadata adds `group`, `individual_genres` and active `credits`.

| Method | Route | Purpose |
| --- | --- | --- |
| POST | `/api/sources/{source}/metadata/group-membership` | JSON `{path, target_path, expected_group_id}`; join a current member's group |
| GET | `/api/sources/{source}/groups/{group}?path=...` | Effective group settings and editability |
| PUT | `/api/sources/{source}/groups/{group}/genres` | JSON `{path, genres:[...]}` |
| DELETE | `/api/sources/{source}/groups/{group}/genres?path=...` | Restore individual tags |
| POST | `/api/sources/{source}/groups/{group}/artwork?path=...` | Raw raster image body, max 8 MiB |
| DELETE | `/api/sources/{source}/groups/{group}/artwork?path=...` | Restore individual artwork |
| GET | `/api/sources/{source}/groups/{group}/artwork?path=...` | Private re-encoded JPEG |

Tests cover folder consolidation and season/disc ordering, ownership boundaries,
new-member inheritance, filename-only episodes, oversized/invalid cover rejection,
revocation and stale membership, independent resets, and provider-specific credits.
Membership tests also cover drag/drop, read-only UI, CSRF, incompatible media,
path traversal, inherited group overrides, preserved individual fields/file bytes,
and audit recording.
