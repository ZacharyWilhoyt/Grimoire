CREATE TABLE media_groups (
    id TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    genres TEXT,
    artwork TEXT REFERENCES metadata_artwork(id),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX media_groups_source ON media_groups(source_id);
