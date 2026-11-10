CREATE TABLE media_comments (
    id BIGINT GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    file_key TEXT NOT NULL,
    author_id UUID REFERENCES users(id) ON DELETE SET NULL,
    parent_id BIGINT,
    body TEXT NOT NULL CHECK (char_length(body) BETWEEN 1 AND 2000),
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE (source_id, file_key, id),
    FOREIGN KEY (source_id, file_key, parent_id)
        REFERENCES media_comments(source_id, file_key, id) ON DELETE CASCADE
);
