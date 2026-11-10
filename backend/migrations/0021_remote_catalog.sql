CREATE TABLE remote_catalog_state (
    source_id TEXT PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE,
    revision BIGINT NOT NULL CHECK (revision > 0),
    digest TEXT NOT NULL,
    published BOOLEAN NOT NULL,
    last_confirmed TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE remote_media_items (
    source_id TEXT NOT NULL REFERENCES remote_catalog_state(source_id) ON DELETE CASCADE,
    id UUID NOT NULL,
    name TEXT NOT NULL CHECK (char_length(name) BETWEEN 1 AND 255),
    PRIMARY KEY (source_id, id)
);

CREATE TABLE remote_media_representations (
    source_id TEXT NOT NULL,
    item_id UUID NOT NULL,
    id UUID NOT NULL,
    extension TEXT NOT NULL,
    size_bytes BIGINT NOT NULL CHECK (size_bytes > 0),
    PRIMARY KEY (source_id, id),
    UNIQUE (source_id, item_id),
    FOREIGN KEY (source_id, item_id) REFERENCES remote_media_items(source_id, id) ON DELETE CASCADE
);

CREATE TABLE remote_media_locations (
    source_id TEXT NOT NULL,
    representation_id UUID NOT NULL,
    node_id UUID NOT NULL REFERENCES source_nodes(id),
    PRIMARY KEY (source_id, representation_id),
    FOREIGN KEY (source_id, representation_id)
        REFERENCES remote_media_representations(source_id, id) ON DELETE CASCADE
);

CREATE TABLE remote_source_grants (
    source_id TEXT NOT NULL REFERENCES remote_catalog_state(source_id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    can_discover BOOLEAN NOT NULL,
    can_access_media BOOLEAN NOT NULL CHECK (NOT can_access_media OR can_discover),
    PRIMARY KEY (source_id, user_id)
);
