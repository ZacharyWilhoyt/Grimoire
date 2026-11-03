# Watch pages, links, and MKV audio

**Current player:** all video playback now uses [segmented transcoding](TRANSCODING.md).
The whole-file compatibility API documented below remains available for older
clients. Source owners control CPU/NVIDIA/AMD encoding; viewers choose quality and bitrate.


Video and EPUB/CBZ open in full-page layouts with a related-content sidebar.
Books keep chapter/page, zoom, text size and theme controls; they have no video
controls. Comments are a disabled visual placeholder. The video fullscreen
button enters/exits the containing stage, keeping the libass subtitle overlay
visible. Playback uses the unified custom control bar.

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

Copies live in a randomly named private directory under `GRIMOIRE_VIDEO_CACHE_DIR`
(default: `GRIMOIRE_DATA_DIR/video-cache`, or `data/video-cache`)
(0700 directory, 0600 files on Unix), never in the source directory. Up to four
completed copies are cached per process with least-recently-used eviction.
`GRIMOIRE_VIDEO_CACHE_MIB` sets the total cache budget (64–65536 MiB; default
8192 MiB). In-flight response bodies retain their storage allocation; preparation
fails busy if insufficient space remains. FFmpeg output is size-limited; allow
filesystem headroom for muxer overhead. Preparation also fails on filesystem
exhaustion. Copies disappear on eviction or graceful process teardown; an abrupt
host/process crash can leave private `grimoire-video-*` temp directories for the
operator to remove from the cache directory while that API process is stopped. Cache state is local to an
API instance: multi-instance deployment will need shared rendition coordination
or affinity. If a copy expires, choose **Prepare audio again**.

## In-player options

The video options bar sits inside the fullscreen stage. Its buttons open episode
preview, subtitle, audio and settings panels; **Cozy view** hides the right sidebar.
Audio and subtitle components remain mounted when their panels close, preserving
selection and ASS rendering. Settings offer playback speed, contain/cover/fill,
and pause when the document is hidden (without automatic resume). Playback, volume, seek and options share the same custom control bar. Filters and upscaling are
not implemented. Original video quality is unchanged; CDN selection and automatic
intro/outro skipping require future backend capabilities.

Processing errors distinguish a failure to start the tool in the API environment
from encoder errors, incompatible MP4 stream copying, temporary storage exhaustion,
size limits and timeouts. A working terminal executable does not establish that
the API service can launch it, and an encoder failure does not mean it is missing.
Diagnostics never return raw FFmpeg stderr (which can contain private file paths).

Playback and options now share one custom control bar: seek timeline above,
rewind/play/forward/mute/volume on the left, options/cozy/fullscreen on the right.
The native browser control bar is disabled. Controls follow media events when
playback is started, paused, ended or its source changes. Keyboard-accessible
buttons and range inputs provide seeking and volume adjustment.

Large MKV remuxes can exceed a memory-backed `/tmp` quota despite having enough
space on the media disk. The default cache now uses the application data disk.
Set an absolute cache directory for a service, ensure its account can write there,
and allow enough actual free space and filesystem quota in addition to the
configured cache budget. Originals and `.ignore` fixtures are never uploaded.

## Player visibility and preferences

Video controls hide after 2.5 seconds of idle playback in both ordinary and
fullscreen mode. Pointer/keyboard activity reveals them. Pausing, an open options
panel, and keyboard focus on controls keep them visible. Pointer focus left on
a clicked button does not pin the bar open indefinitely.

On desktop, the stage fills the main column beside related content. Cozy view
fills the viewport without outer padding or a title header; Back moves into the
control bar. Video uses its selected fit mode (Contain by default), so aspect
ratio letterboxing can remain. Mobile ordinary view stacks related content below
the video; cozy still fills the viewport.

Playback resolution and subtitle track label/format are saved in browser local storage, scoped by account ID.
They survive reloads and moving to another episode on the same browser, but do
not synchronize between devices. Original stream indexes are not reused between
files. A subtitle preference is restored only if a matching label/format exists;
otherwise subtitles stay off. Choosing Off saves that preference. Encoder policy is stored server-side per source and cannot be overridden by viewer preferences. Storage errors
or blocked storage leave playback usable with session-only settings.

## Episode navigation and mini-player

Video playback offers **Next episode** when the permission-filtered series context
has a following member, and advances to that member when playback ends. Ordering
uses the existing series/season/episode metadata. Browser autoplay restrictions can
still require pressing Play on the next item.

Back from the video page minimizes playback into the bottom-right corner while
browsing the library. The same media element and subtitle renderer stay mounted.
The mini-player has restore (top left), stop (top right), play/pause (center), and
progress along the bottom. Starting music, opening another media item, signing out,
or losing source access closes the old video; it does not survive a page reload.

## Keyboard shortcuts and mini-player controls

Mini-player buttons appear on pointer/focus activity and fade after 2.5 seconds
of inactivity; the progress bar remains visible and keyboard focus remains usable.
Video shortcuts ignore text fields and selects: F fullscreen, K/Space play/pause,
J/L seek ten seconds, Left/Right seek five seconds, Up/Down adjust volume, M mute,
0–9 seek to a percentage, comma/period step a frame while paused, < / > change
speed, Shift+N next episode, C subtitle options, and T cozy view.

## Reading layouts and profiles

The reader keeps page navigation in the top header while only the content viewport
scrolls. A cog in the upper-right content margin opens reading settings; Escape,
Close, or clicking outside dismisses it. Zoom is controlled by Shift + wheel,
anchored to the content point under the cursor, including inside EPUB chapters.
Double click content (or use the header toggle) for cozy view. Left/right edge clicks
navigate even when a page fills the viewport; Manga defaults to right-to-left, with
an explicit reading-direction setting. Shift + wheel zooms from 25–400%; drag to pan. The header navigation reveals on
hover or keyboard focus in cozy view.

CBZ supports original size, fit width, fit height, and fit both, plus single page,
double page, and long strip. Double-page layout pairs two confirmed portrait images;
images wider than 1.2 times their height are treated as full spreads and shown alone.
This is a dimension heuristic, not semantic detection of artwork gutters. Single page
is available for ambiguous scans. Long strip loads resources near the viewport and
releases rendered resources farther away while retaining placeholder heights.

EPUB always uses scrolling chapters with adjustable font size/theme. Chapters retain
sanitization, a restrictive CSP, and an iframe sandbox that forbids scripts. The sandbox
allows same-origin access solely for parent-controlled sizing and input forwarding;
book scripts remain blocked by both sandbox and CSP. Chapter iframes expand to their
content height so scrolling belongs to the reader viewport.

Novel, Comic, Manga, and Manhwa are user-selectable profiles. Fit, layout, zoom,
font size, theme, and direction are saved per account/profile in browser storage;
each book's chosen profile is saved separately, and new books use the last selected
profile for their format. Manhwa defaults to fit-width long
strip, Manga to right-to-left. Settings do not synchronize between devices.

At the end of a volume, Next or scrolling to the end of long strip opens the next
entry from the permission-filtered series. No access is granted by adjacency. Opening a book stops the music player and closes its queue.

UI margins, padding, and gaps are reduced by 10% throughout the stylesheet.
Structural sidebar alignment offsets, media dimensions, and font sizes are preserved.
