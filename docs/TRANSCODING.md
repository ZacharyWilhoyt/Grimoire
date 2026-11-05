# CPU and GPU video transcoding

Playback always starts as segmented transcoding. In the player, open
**Settings → Playback quality** and choose **Highest available** or a lower
resolution, then **Apply quality**. Audio and subtitle choices are displayed
as scrollable button lists. There is no direct/original playback option.
Source owners select the encoder under their source’s **Source video encoding**
settings; viewers cannot override it. Existing and new sources default to CPU. Output is H.264 Main, 8-bit 4:2:0, up to
30 fps, with stereo 48 kHz AAC at 192 kbps. Original files are never modified.
Subtitles stay separate: ASS continues to use libass and original attachments.
Progress remains associated with the original file and its full timeline.

## Encoder support

- CPU: FFmpeg `libx264` (veryfast preset).
- NVIDIA: `h264_nvenc`, NVENC-capable GPU and matching driver/runtime.
- AMD on Linux: `h264_vaapi`, Mesa/libva video encoder support and permission
  for the render device. `GRIMOIRE_VAAPI_DEVICE` defaults to `/dev/dri/renderD128`.
  VA-API also works on other vendors supported by their drivers.
- AMD AMF: `h264_amf` when FFmpeg and the host's AMF runtime support it.

The API executes a short real encode for each backend. Compiled-in encoders
without a usable device are marked unavailable. Results are cached until API
restart. Auto prefers NVENC, then VA-API, then AMF, then CPU among passing
backends. If an encoder fails on the actual input, choose CPU manually; it does
not silently mix encoder implementations mid-stream. Input decoding/scaling
currently run on the CPU; GPU backends accelerate video encoding. This is not
a complete hardware decode/filter pipeline. Drivers, containers and device access
are operator configuration; the application does not install drivers or change
system device permissions.

Vendor references: [NVIDIA FFmpeg guide](https://docs.nvidia.com/video-technologies/video-codec-sdk/13.1/ffmpeg-with-nvidia-gpu/index.html),
[AMD AMF integration](https://github.com/GPUOpen-LibrariesAndSDKs/AMF/wiki/FFmpeg-and-AMF-HW-Acceleration).

## Container GPU access

The base Compose deployment passes only `/dev/dri/renderD128` to the API,
adds its render group, and includes Mesa VA-API drivers. It keeps the non-root
API user, dropped capabilities, read-only root filesystem and source-owner
encoding permissions. Existing sources continue using their configured encoder.
No privileged container or Docker socket mount is needed.

On this host the render group is GID **110**. On another host, inspect
`stat -c '%g' /dev/dri/renderD128` and set `GRIMOIRE_RENDER_GID` in `deploy/.env`.
`GRIMOIRE_VAAPI_HOST_DEVICE` can select a different host render node; inside the
container it is always `/dev/dri/renderD128`. Rebuild/recreate the API to apply:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml up -d --build --no-deps api
```

A small synthetic encode checks real hardware access without reading media:

```sh
docker compose --env-file deploy/.env -f deploy/compose.yaml exec -T api ffmpeg -hide_banner -loglevel error -vaapi_device /dev/dri/renderD128 -f lavfi -i color=size=128x128:rate=1 -vf format=nv12,hwupload -frames:v 1 -c:v h264_vaapi -f null -
```

For a host **without** a render node, append
`-f deploy/compose.cpu.yaml` to every Compose command (before the subcommand).
This removes device/group mappings; CI uses this override too.

For **NVIDIA**, install the host NVIDIA driver and NVIDIA Container Toolkit,
configure Docker's NVIDIA runtime, and append `-f deploy/compose.nvidia.yaml`
instead. It removes AMD mappings and requests one NVIDIA GPU with video/compute
runtime libraries. This configuration requires a working host runtime; it has
not been hardware-tested on this AMD host. Source owners must still explicitly
select NVIDIA/Auto encoding. Do not use the CPU and NVIDIA overrides together.
Compose v2.24.4+ is required for the `!reset` overrides.

## Playback and bounded processing

The server generates authenticated VOD HLS playlists and encodes independent
six-second MPEG-TS segments on demand. Seeking requests segments at the desired
position, without converting the intervening file. Browser playback uses lazy-loaded
hls.js, with native HLS as a fallback. A short forward/back buffer limits work.
This avoids the previous whole-file MP4 copy, its multi-GiB disk requirements,
and the wait for conversion to finish. The old audio-copy endpoint remains for
compatibility, but the player audio selector now uses segmented transcoding.

This pipeline offers manual quality profiles, not adaptive multi-bitrate switching.
Video targets are 1 Mbps up to 480p, 2.5 Mbps up to 720p, 5 Mbps up to 1080p,
10 Mbps up to 1440p and 20 Mbps up to 2160p; AAC audio adds up to 192 kbps.
These are variable bitrate targets, not guaranteed network rates. CPU encoding
uses bitrate control rather than the previous CRF setting. Targets are displayed
next to each resolution and in the active-playback summary. Six-second segments
restart encoders at boundaries; gapless/sample-perfect audio is not guaranteed.
There is no HDR tone mapping, filters, upscaling, deinterlacing policy or hardware
video decoding yet. HDR inputs may have incorrect colors. Inputs up to 24 hours
are supported. Highest available preserves source resolution up to 2160p (4K), without upscaling.
Source containers: MKV/WebM, MP4/MOV/M4V, AVI, MPEG and MPEG-TS.
Encoders and scaling syntax must be supported by the installed FFmpeg version.

Two segment/probe jobs run at once per API process; a segment has a 90-second
timeout, 24 MiB output limit, and two CPU encoding/filter threads. The process
caches eight completed segments (up to 192 MiB, plus active response buffers and
FFmpeg memory). It writes no transcoded video to disk. FFmpeg allocations are
capped per allocation, not total RSS; deployment should enforce process/container
memory limits for hostile media. Clients can receive busy responses when both
workers are occupied. Work in progress ends when its request is cancelled or
hits its bound; stopping a session prevents serving its pending result.

Sessions expire after 30 minutes without requests and are limited to four per
account and 32 per API instance. Session creation is limited to 30 per account
per five minutes; segment requests to 600 per five minutes. These are processing
safeguards, not acquisition policies or storage quotas. Abandoned sessions expire;
leaving the player deletes its session. Each
API instance has its own sessions/cache, so multiple instances need request
affinity or future shared coordination. A paused session may need restarting.

## Authorization and API

- `GET /api/transcode/capabilities`: encoder availability, restricted to accounts
  owning a source. CPU-only playback never probes GPU backends.
- `GET/PUT /api/sources/{id}/transcoding`: source-owner-only encoder policy; PUT
  takes `encoder` and requires CSRF/Origin checks.
- `GET /api/transcode/qualities?source_id=...&path=...`: current source resolution
  and bitrate profiles; requires media access.
- `POST /api/transcode`: JSON `source_id`, `path`, `height` (0 for highest available, or 480/720/1080/1440/2160), `audio` (original stream index
  or null for default). Requires normal Origin/CSRF checks.
- `GET /api/transcode/{id}/manifest.m3u8` and
  `GET /api/transcode/{id}/segment?index=N`.
- `DELETE /api/transcode/{id}`: end only your own session; requires CSRF.

The viewer request rejects an `encoder` field. The backend loads the source
owner’s policy, validates it again before session creation, and on every playlist
and segment request. Policy changes invalidate existing sessions, including cached
segments. Already-running jobs may finish but their results are not served.

Session IDs do not grant access: playlists and segments require the logged-in
session owner and current source media permissions. Every request revalidates
original path, file identity and authorization, including cached segments.
Authorization and fingerprint are checked again after encoding before serving.
Responses are private/no-store. Revocation does not erase media already buffered
in a viewer's browser. Inputs use fixed demuxers/protocols and server-owned
encoder flags; clients cannot supply FFmpeg arguments or remote media URLs.

## Validation

CPU encode, segment timestamps/codecs, full-duration manifest, user isolation,
CSRF and revocation are covered by the ignored PostgreSQL/FFmpeg integration test.
CPU and VA-API playback, seeking and ASS were exercised in the browser on the
development host. NVENC/AMF were unavailable there, so real encoding on those
backends still needs validation on hosts with compatible hardware/drivers.
The private media fixture remains ignored and is never part of tests committed
to the repository.

### Segment continuity

The browser buffers up to 36 seconds ahead (60-second upper limit). Segment duration
is 6.016 seconds, exactly 282 AAC frames at 48 kHz. After the first segment, encoding
includes 256 ms of preceding audio to warm the AAC encoder; video retains its own
segment boundary and HLS removes overlapping audio by timestamp. Video frame-rate
conversion occurs before trimming, with timestamp passthrough to avoid duplicated
frames shifting the cut. These changes reduce buffer stalls and fresh-encoder audio
artifacts; real hardware/browser listening remains useful for codec-specific issues.
