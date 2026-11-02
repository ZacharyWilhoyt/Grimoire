CREATE TABLE user_media (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    file_key TEXT NOT NULL CHECK (length(file_key) = 64),
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    saved_at TIMESTAMPTZ,
    last_opened_at TIMESTAMPTZ,
    PRIMARY KEY (user_id, file_key)
);
CREATE INDEX user_media_saved ON user_media(user_id, saved_at DESC) WHERE saved_at IS NOT NULL;
CREATE INDEX user_media_recent ON user_media(user_id, last_opened_at DESC) WHERE last_opened_at IS NOT NULL;
CREATE INDEX user_media_source ON user_media(source_id);
