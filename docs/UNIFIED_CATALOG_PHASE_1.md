# Unified catalog Phase 1 foundations

The operational Source Agent branch was merged into GitHub main before the
foundation change. The live database's 21 applied migration checksums were
compared read-only and match the reconciled tree.

Migration `0022_unified_catalog_foundations.sql` adds logical items,
representations, locations, typed identity evidence and logical ID aliases.
The existing remote index remains the authoritative source-local record. The
new location table can reference its representation once Phase 2 starts
projecting agent deltas. Existing local file keys and paths are retained as
location references. The migration leaves existing rows and route contracts
in place.

The local inventory bridge records observations from authorized Discover and
source browse scans. A single background writer processes batches of up to
256 items, with a two-second deadline per batch. PostgreSQL serializes
initial assignment so repeat observations preserve the logical and
representation IDs. A scan that is skipped, fails, or returns a partial
result does not remove locations. The observed size and format seed the
initial representation. A future matching phase will handle file replacement
and byte identity explicitly.

The inventory fills incrementally when existing routes scan files. The
legacy catalog still serves their normal live results, including files
that the inventory has yet to observe. Phase 1 introduces no automatic
identity associations, catalog deduplication or playback selection.

The upgrade fixture checks that an existing file retains its key, progress
and metadata, that its Arr connection and comment survive migration, and
that a second observation preserves the new inventory IDs.

Strict Clippy, 120 frontend tests, the frontend production build, 36 Rust
unit tests and 39 integration tests passed. The disposable HTTPS smoke test
verified agent enrollment, indexing, multi-chunk seeking, re-enrollment,
and local denial. The production coordinator has not applied migration 0022.
