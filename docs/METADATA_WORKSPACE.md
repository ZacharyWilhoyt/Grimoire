# Metadata workspace

Open **Metadata** from the sidebar and select a source you own. Choose up to
50 indexed files of the same media type, search for an existing collection,
and select **Add to collection**. The collection selector includes sources
you can browse. For a file that has just been scanned, refresh if indexing
has not yet made it selectable.

A collection link places the file in the selected series or album while
keeping its title, numbering, logical item ID, and progress. Select the files
and use **Remove collection links** to restore their source-local grouping.
A linked entry appears in the cross-source collection only to viewers who
can see both sides. A deleted or replaced anchor invalidates the association.
If a collection changes identity, select its current group again.

Metadata edits, poster and tag edits, and metadata scans for local sources
are available from the same page. For a single local file in the same source
as its target, **Copy group metadata** uses the existing source-local
operation. Source Agent files can be assigned to collections. Their
metadata is still managed through the existing Source Agent and provider
workflows.

Migration 0026 adds the collection membership table. The API validates
ownership of each changed file, target visibility, and media type before
applying a batch. Collection membership is independent of deduplication and
playback routing.
