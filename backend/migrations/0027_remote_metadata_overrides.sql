CREATE TABLE catalog_remote_metadata_overrides (
    representation_id uuid PRIMARY KEY REFERENCES catalog_representations(id) ON DELETE CASCADE,
    fields jsonb NOT NULL,
    group_id text CHECK (group_id ~ '^[0-9a-f]{64}$'),
    hide_artwork boolean NOT NULL DEFAULT false
);
CREATE INDEX catalog_remote_metadata_group ON catalog_remote_metadata_overrides(group_id);
