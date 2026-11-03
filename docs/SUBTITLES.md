# Video subtitles

The video player starts with subtitles **Off**. Choose a track from **Subtitles**.

- Neighboring UTF-8 `.srt`, `.vtt`, `.ass` and `.ssa` files are found when their
  stem matches the video or starts with its stem plus a dot, such as
  `Episode.mkv` with `Episode.en.ass`.
- MKV text subtitle streams are discovered using FFprobe. ASS/SSA is extracted
  without flattening styling; SubRip/WebVTT/text is converted to WebVTT.
- ASS uses JASSUB 2.4.1's libass WebAssembly renderer. Original script styles,
  positioning, drawing, animation and karaoke commands reach libass intact.
  Matroska TTF/OTF font attachments are loaded before renderer construction.
  Missing fonts use its bundled Liberation Sans fallback, so exact typography
  still requires the correct fonts in the file. Remote/local-system font lookup
  is disabled. Sidecar scripts can use script-embedded fonts or this fallback;
  arbitrary neighboring font directories are not read.
- Subtitle scripts use their original timing; there is no manual delay control.
  Use **Fullscreen with subtitles** to include the canvas overlay. Native video
  fullscreen and picture-in-picture can omit it.

FFmpeg and FFprobe must be installed on the API host and available on PATH for
embedded tracks. Sidecar files do not require them. Modern browsers need
WebAssembly, workers and OffscreenCanvas; unsupported environments report a
rendering failure. Cross-origin isolation is optional for JASSUB's single-thread
fallback. See [video transcoding](TRANSCODING.md) for browser-compatible
H.264/AAC playback. Bitmap PGS/VobSub tracks are not implemented and
produce a warning rather than an incorrect text conversion.

## Boundaries and limits

All listing, script and font requests require the same source media permission as
video playback, rechecked after processing. Visibility alone is insufficient.
Paths use the existing no-traversal/no-symlink resolver. Sidecars must belong to
the requested video. Font and subtitle stream indexes are checked against the
actual MKV. Responses use private/no-store and nosniff headers.

Matroska processing forces the Matroska demuxer rather than autodetecting arbitrary
playlists, passes filenames as arguments without a shell, and disables network
protocols. Extraction reads the seekable original file; it does not silently stop
after a prefix of a large movie. Two jobs may run per API process; requests are
rate limited per account, each process has a 120-second deadline and bounded
output. Limits: 128 streams, 32 displayed tracks, 16 fonts, 8 MiB per font,
32 MiB total browser font data, 16 MiB ASS/SSA scripts, 1 MiB SRT/WebVTT
scripts, 5,000 sidecar directory
entries, 10,000 normalized WebVTT cues. Exceeding limits fails or warns explicitly.
FFmpeg's internal allocations are not an OS memory sandbox; production processing
of hostile media should run in a restricted worker/container with OS resource
limits. There is no persistent subtitle extraction cache yet, so large files may
require a scan when selecting a track.

SRT/WebVTT are normalized to plain-text native cues; custom WebVTT styles and cue
position settings are intentionally omitted. ASS is never passed through that
converter. Selected tracks apply to this player session. Closing/switching
tracks cancels fetches, releases blob URLs, and destroys the libass worker.

## Verification

`cargo test` covers parser/path/codec rules. `cargo test -- --ignored` includes
PostgreSQL permission/revocation tests and an FFmpeg fixture test that preserves
ASS position/karaoke/vector commands and extracts an attached font byte-for-byte.
The latter needs FFmpeg and `fonts-dejavu-core` (CI installs both).
Frontend tests check explicit selection, untouched scripts/fonts, disabled remote
font lookup and renderer cleanup. Browser verification used a synthetic MKV with
positioned yellow text, vector drawing, italics and karaoke, including fullscreen.
This is not a claim that every fansub script/browser/GPU combination is verified.

JASSUB source, build recipes and component licensing are provided by the
[upstream project](https://github.com/ThaUnknown/jassub). The wrapper's notice is
served at `/licenses/jassub.txt`; upstream includes libass/FreeType/HarfBuzz and
other compiled dependencies with their own terms. Preserve those terms and source
availability when redistributing renderer binaries.

Video retries replace the media element; subtitle cleanup also removes its owned
canvas overlay if the original video has already been detached. The selected
track is reloaded against the new video element without leaving old overlays.

Modern Matroska `font/ttf` and `font/otf` attachment MIME types are recognized
even when FFprobe omits a font codec name. Extracted attachments must have a
supported TrueType/OpenType header. The larger ASS limit accommodates vector
drawings and extensive typesetting while retaining bounded output and renderer
limits. API errors are shown in the subtitle panel rather than replaced by an
ambiguous loading/limit message. Regression fixtures exercise scripts above
1 MiB and modern font MIME tags; private source scripts are not committed.

Successful embedded script/font extractions are cached in process for ten minutes,
with at most 32 entries and 48 MiB total. File path, size, modification time, stream,
and format distinguish cache entries. Every HTTP request still checks current
permissions. Two font downloads run concurrently, with the existing 32 MiB client
font budget. ASS headers/styles, original timestamps, and overlapping events are preserved.
Embedded ASS initially requests `preview=true`, a copy-only extraction of the
opening 120 seconds, then fetches the full track in the background. This avoids
waiting for a complete large remote MKV scan before displaying the opening scenes.
Seeking beyond the preview before extraction finishes can still require a wait;
the panel reports that state and any background extraction failure.

The client feeds libass rolling time windows: every 60-second interval includes
events overlapping the preceding 30 and following 120 seconds. Long-running signs
remain included for their full duration; no timing, drawing, karaoke, animation,
style, or font data is simplified. Unknown event formats/timestamps fall back to
the complete script. Seeks update the window without restarting font loading.
The current track and fonts are retained only in the mounted player's memory,
within the existing script/font limits. Track switches reuse fonts for that file.

MKV inspection is cached for ten minutes (32 entries, each bounded to 256 KiB),
and concurrent inspections/extractions of the same file/track share their work.
Successful extraction caching remains limited to 48 MiB/32 entries. File identity
keys include path, size, and modification time. Authentication, stream validation,
and current source permission checks still happen for each HTTP request. Renderer
module, preview, and font loading overlap; font extraction copies streams instead
of decoding video. A complete first extraction still scans the container in the
background and retains the existing processing/output limits.

JASSUB 2.4.1 ships a source map for its WASM JavaScript loader that references an
unpublished source file. Vite strips only that loader's broken source-map comment
through a targeted development/worker build plugin. Worker code, WASM assets, and
application source maps are preserved. Restart Vite after updating its configuration.
