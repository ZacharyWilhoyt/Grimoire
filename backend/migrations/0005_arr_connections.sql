CREATE TABLE arr_connections (
    id UUID PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK(kind IN ('sonarr','radarr','lidarr')),
    base_url TEXT NOT NULL,
    remote_root TEXT NOT NULL,
    source_subpath TEXT NOT NULL DEFAULT '',
    api_key_encrypted BYTEA NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    revision UUID NOT NULL,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX arr_connections_source ON arr_connections(source_id);
ALTER TABLE metadata_reviews ADD COLUMN arr_connection_id UUID REFERENCES arr_connections(id) ON DELETE CASCADE;
ALTER TABLE metadata_reviews ADD COLUMN arr_revision UUID;
