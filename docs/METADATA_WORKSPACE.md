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

## TheTVDB for TV and anime

Configure a TheTVDB v4 project API key that permits use by your deployment.
Set it as `GRIMOIRE_TVDB_API_KEY` in the API's private environment. Compose
passes it from `deploy/.env` to the API. The key stays server-side. Grimoire
uses the project's key without requiring a Sonarr installation or exposing
credentials to viewers. The existing online metadata toggle still applies.
See [TheTVDB API and attribution](https://thetvdb.com/api-information).

On **Metadata**, edit an owned TV file and choose **TheTVDB television**.
Search for a series, select the aired, DVD, or absolute order, and enter
the season and episode number if needed. Preview the episode before
applying it. This works for local and Source Agent video files. Grimoire
stores series ID, episode ID, and order with the reviewed match and uses
English translations where available. A series poster is requested during
apply. The matched record preserves custom overrides unless the owner
selects the replace option.

The workflow matches individual files. It does not scan or acquire media
and does not automatically merge independently owned files into a shared
canonical identity. Migration 0028 stores reviewed TheTVDB matches for
Source Agent representations.

For TMDB, configure either the v3 key as `GRIMOIRE_TMDB_API_KEY` or the
API Read Access Token as `GRIMOIRE_TMDB_TOKEN` in the API's private
environment. The token takes precedence if both are present.
