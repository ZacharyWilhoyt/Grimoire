CREATE TABLE catalog_items (
    id UUID PRIMARY KEY,
    kind TEXT NOT NULL CHECK (kind IN ('video', 'music', 'book')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE catalog_representations (
    id UUID PRIMARY KEY,
    item_id UUID NOT NULL REFERENCES catalog_items(id) ON DELETE RESTRICT,
    extension TEXT NOT NULL CHECK (extension ~ '^[a-z0-9]{1,12}$'),
    size_bytes BIGINT NOT NULL CHECK (size_bytes >= 0),
    content_sha256 TEXT CHECK (content_sha256 ~ '^[0-9a-f]{64}$')
);
CREATE INDEX catalog_representations_item ON catalog_representations(item_id);

CREATE TABLE catalog_locations (
    id UUID PRIMARY KEY,
    representation_id UUID NOT NULL REFERENCES catalog_representations(id) ON DELETE RESTRICT,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    local_file_key TEXT UNIQUE CHECK (local_file_key ~ '^[0-9a-f]{64}$'),
    local_relative_path TEXT,
    remote_representation_id UUID,
    last_observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (
        (local_file_key IS NOT NULL AND local_relative_path IS NOT NULL AND remote_representation_id IS NULL)
        OR (local_file_key IS NULL AND local_relative_path IS NULL AND remote_representation_id IS NOT NULL)
    ),
    UNIQUE (source_id, remote_representation_id),
    FOREIGN KEY (source_id, remote_representation_id)
        REFERENCES remote_media_representations(source_id, id) ON DELETE CASCADE
);
CREATE INDEX catalog_locations_source ON catalog_locations(source_id);
CREATE INDEX catalog_locations_representation ON catalog_locations(representation_id);

CREATE TABLE catalog_identity_evidence (
    id UUID PRIMARY KEY,
    location_id UUID NOT NULL REFERENCES catalog_locations(id) ON DELETE CASCADE,
    origin TEXT NOT NULL,
    namespace TEXT NOT NULL,
    entity_type TEXT NOT NULL,
    identifier TEXT NOT NULL,
    verification TEXT NOT NULL CHECK (verification IN ('asserted', 'verified', 'rejected')),
    observed_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (location_id, origin, namespace, entity_type, identifier)
);

CREATE TABLE catalog_item_aliases (
    previous_id UUID PRIMARY KEY REFERENCES catalog_items(id) ON DELETE RESTRICT,
    canonical_id UUID NOT NULL REFERENCES catalog_items(id) ON DELETE RESTRICT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    CHECK (previous_id <> canonical_id)
);
