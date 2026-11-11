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
operation. Owners of published Source Agent files can edit display metadata,
hide provider artwork, and reset overrides from the same editor. These edits
apply to the indexed representation and preserve the file, canonical
identity, and existing collection links. Agent-side tag extraction and
provider matching use their existing workflows.

Migration 0026 adds the collection membership table. The API validates
ownership of each changed file, target visibility, and media type before
applying a batch. Collection membership is independent of deduplication and
playback routing.

Migration 0027 adds Source Agent metadata overrides.
