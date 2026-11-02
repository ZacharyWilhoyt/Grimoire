CREATE TABLE media_progress (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    file_key TEXT NOT NULL,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    position DOUBLE PRECISION NOT NULL CHECK(position >= 0 AND position <= 31536000),
    total DOUBLE PRECISION CHECK(total > 0 AND total <= 31536000),
    locator TEXT,
    revision BIGINT NOT NULL CHECK(revision > 0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(user_id,file_key)
);
