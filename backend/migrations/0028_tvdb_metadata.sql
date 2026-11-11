CREATE TABLE catalog_remote_tvdb_matches (
    representation_id uuid PRIMARY KEY REFERENCES catalog_representations(id) ON DELETE CASCADE,
    provider_id text NOT NULL,
    group_id text NOT NULL CHECK (group_id ~ '^[0-9a-f]{64}$'),
    fields jsonb NOT NULL,
    artwork text REFERENCES metadata_artwork(id)
);
CREATE INDEX catalog_remote_tvdb_group ON catalog_remote_tvdb_matches(group_id);
