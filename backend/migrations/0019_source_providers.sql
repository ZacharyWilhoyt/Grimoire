CREATE TABLE source_nodes (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (id, owner_id)
);
CREATE INDEX source_nodes_owner ON source_nodes(owner_id);

-- NULL paths are reserved for remote sources. The existing path uniqueness
-- constraint still applies to all local directories.
ALTER TABLE sources
    DROP CONSTRAINT sources_kind_check,
    ALTER COLUMN path DROP NOT NULL,
    ADD COLUMN node_id UUID,
    ADD CONSTRAINT sources_node_owner
        FOREIGN KEY (node_id, owner_id) REFERENCES source_nodes(id, owner_id),
    ADD CONSTRAINT sources_provider_shape CHECK (
        (kind = 'local_directory' AND path IS NOT NULL AND node_id IS NULL) OR
        (kind = 'remote_agent' AND path IS NULL AND node_id IS NOT NULL)
    );
CREATE INDEX sources_node ON sources(node_id) WHERE node_id IS NOT NULL;
