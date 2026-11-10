ALTER TABLE source_nodes
    ADD COLUMN label TEXT NOT NULL DEFAULT 'Source Agent' CHECK (char_length(label) BETWEEN 1 AND 80),
    ADD COLUMN public_key BYTEA CHECK (octet_length(public_key) = 32),
    ADD COLUMN last_seen TIMESTAMPTZ,
    ADD COLUMN last_request_ms BIGINT,
    ADD COLUMN revoked_at TIMESTAMPTZ;

CREATE TABLE source_node_enrollments (
    token_hash TEXT PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    expires_at TIMESTAMPTZ NOT NULL
);
CREATE INDEX source_node_enrollments_expiry ON source_node_enrollments(expires_at);

CREATE TABLE source_node_events (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    node_id UUID NOT NULL REFERENCES source_nodes(id),
    actor_id UUID NOT NULL REFERENCES users(id),
    event TEXT NOT NULL CHECK (event IN ('enrolled', 'rotated', 'revoked')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE UNIQUE INDEX source_nodes_public_key ON source_nodes(public_key) WHERE public_key IS NOT NULL;
