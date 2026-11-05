-- Personal intent/favorites do not change source metadata, progress, or sharing.
CREATE TABLE user_series (
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    group_id TEXT NOT NULL CHECK(length(group_id)=64),
    status TEXT CHECK(status IN ('planning','active','completed','on_hold','dropped')),
    favorite BOOLEAN NOT NULL DEFAULT false,
    revision BIGINT NOT NULL CHECK(revision>0),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    PRIMARY KEY(user_id,source_id,group_id)
);
