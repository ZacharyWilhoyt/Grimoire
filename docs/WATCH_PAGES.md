# Watch pages, links, and MKV audio

Video and EPUB/CBZ open in full-page layouts with a related-content sidebar.
Books keep chapter/page, zoom, text size and theme controls; they have no video
controls. Comments are a disabled visual placeholder. The video fullscreen
button enters/exits the containing stage, keeping the libass subtitle overlay
visible. Native video controls remain available.

## Sharing links

The link icons in catalog cards/folders and the player sidebar are ordinary
copyable links. `/?source=SOURCE&item=ENCODED_PATH` opens an item;
`/?source=SOURCE&series=GROUP_ID` opens its album/series. Login preserves the
requested URL. Browser Back/Forward resolves it again for the current account.
`GET /api/media/link?source_id=...&path=...` (or `group=...`) resolves the
current source catalog, metadata group and members with live media permission
checks. A URL never grants permissions. Series membership uses existing
source-scoped metadata grouping; changing its identifying collection/creator
can change the group URL. Renaming/moving actual files changes item URLs.

## Audio selection

MKV audio tracks are inspected with FFprobe and listed by language/title/codec.
Selecting a track prepares a seekable MP4: FFmpeg copies the first video stream
unchanged and converts only the selected audio stream to stereo AAC at 192 kbps.
This handles common browser-incompatible AC3/DTS audio as well as track choice.
Original/default audio remains available without preparing a copy. ASS scripts
and attached fonts continue to come from the original MKV through libass.
Progress remains attached to the original item; switching saves/flushes the old
position before loading progress for the new resource. Volume, mute and speed
are preserved. Preparing a copy pauses playback and can take minutes.

FFmpeg and FFprobe must be installed on the API host. This does not transcode
video: unsupported video codecs/MP4 combinations can still fail. There is no
HLS/adaptive quality pipeline or automatic next-episode playback.

Authenticated endpoints:

- `GET /api/sources/{id}/audio-tracks?path=...`
- `POST /api/sources/{id}/video-audio?path=...`, JSON `{"stream_index":2}`
- `GET /api/sources/{id}/video-audio/{rendition}?path=...`, including ranges

Preparation requires the usual Origin and CSRF protections, current media
permission and a validated stream index. Every range/download rechecks current
media access and the original file fingerprint. Copies cannot authorize access
to a different source/file. Source changes invalidate copies.

## Resource limits

One audio preparation runs per API process, with a five-minute encoding timeout.
Preparation is limited to 20 attempts per user per five minutes; audio inspection
to 60 per five minutes. Existing subtitle inspection/extraction limits also apply.
Demuxers and subprocess protocols are restricted; arbitrary remote URLs are not
accepted. A completed output's duration is checked to reject size-limited,
incomplete outputs.

Copies live in a randomly named private directory under the system temp directory
(0700 directory, 0600 files on Unix), never in the source directory. Up to four
completed copies are cached per process with least-recently-used eviction.
`GRIMOIRE_VIDEO_CACHE_MIB` sets the total cache budget (64–65536 MiB; default
8192 MiB). In-flight response bodies retain their storage allocation; preparation
fails busy if insufficient space remains. FFmpeg output is size-limited; allow
filesystem headroom for muxer overhead. Preparation also fails on filesystem
exhaustion. Copies disappear on eviction or graceful process teardown; an abrupt
host/process crash can leave private `grimoire-video-*` temp directories for the
operator to remove while that API process is stopped. Cache state is local to an
API instance: multi-instance deployment will need shared rendition coordination
or affinity. If a copy expires, choose **Prepare audio again**.
