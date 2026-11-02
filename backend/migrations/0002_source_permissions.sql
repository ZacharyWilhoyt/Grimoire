CREATE TABLE source_permissions (
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    can_discover BOOLEAN NOT NULL DEFAULT FALSE,
    can_access_media BOOLEAN NOT NULL DEFAULT FALSE,
    acquisition_policy TEXT NOT NULL DEFAULT 'auto_deny'
        CHECK (acquisition_policy IN ('auto_deny', 'manual_approval', 'auto_accept')),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY (source_id, user_id),
    CHECK (can_discover OR (NOT can_access_media AND acquisition_policy = 'auto_deny'))
);
CREATE INDEX source_permissions_user ON source_permissions(user_id);
-- No source FK here: preserve the audit trail after a source is disconnected.
CREATE TABLE source_permission_audit (
    id BIGSERIAL PRIMARY KEY,
    source_id TEXT NOT NULL,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    target_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL CHECK (action IN ('updated', 'revoked')),
    before_state TEXT,
    after_state TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX source_permission_audit_source ON source_permission_audit(source_id, created_at);
