# Book archive loading

CBZ supports archives up to **4 GiB**, with up to 8 GiB of declared expanded
resources. EPUB retains its 128 MiB archive / 256 MiB expanded limits. Both reject
encryption, duplicate filenames, more than 5,000 entries, and entries larger than
32 MiB. ZIP index reads are bounded to 16 MiB. These are processing limits, not
claims that every archive within them will decode successfully.

The API streams the original file through its existing authorized, private,
no-store endpoint. Byte ranges are checked against source/media access on every
request. No full-file read into backend memory is introduced.

The browser reads the ZIP index and requested pages using bounded HTTP ranges;
it never constructs a Blob of the entire archive. Responses must be 206 with the
expected Content-Range, size and byte count. Full-file fallback responses are
cancelled, not downloaded. If available, an ETag or Last-Modified validator is
sent with If-Range to reject files changed while reading. Range requests use
same-origin cookies and no-store caching. Proxies must preserve these headers
and partial responses without altering the bytes.

At most two resources extract concurrently. Each extraction checks actual output
size and the ZIP CRC. Extracted Blob resources use a **64 MiB LRU cache** rather
than accumulating for every visited page; revisiting an evicted resource fetches
it again. Closing the reader aborts pending archive requests and clears its cache.
The existing reader releases object URLs for offscreen strip pages. Browser
image decoding, active pages and temporary extraction buffers use additional
memory; 64 MiB is the archive cache budget, not a total browser RAM limit.

Automatic metadata/cover gathering has its own smaller archive limits, documented
in [Metadata](METADATA.md). Large CBZ books may require manual metadata/artwork.

Tests cover a sparse archive above 128 MiB, bounded network reads, cache eviction,
concurrent request deduplication, rejected full responses, size/expansion limits,
and streaming a byte of a sparse 4 GiB file from the backend.
