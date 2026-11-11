ALTER TABLE media_comments
    ADD COLUMN removed_at TIMESTAMPTZ,
    ADD COLUMN removed_by UUID REFERENCES users(id) ON DELETE SET NULL;
