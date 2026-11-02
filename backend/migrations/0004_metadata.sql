CREATE TABLE metadata_artwork (
    id TEXT PRIMARY KEY,
    jpeg BYTEA NOT NULL CHECK (octet_length(jpeg) <= 524288)
);
CREATE TABLE media_metadata (
    file_key TEXT PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    relative_path TEXT NOT NULL,
    local_data TEXT NOT NULL DEFAULT '{}',
    matched_data TEXT NOT NULL DEFAULT '{}',
    custom_data TEXT,
    provider TEXT,
    provider_id TEXT,
    local_artwork TEXT REFERENCES metadata_artwork(id),
    matched_artwork TEXT REFERENCES metadata_artwork(id),
    artwork_preference TEXT NOT NULL DEFAULT 'auto' CHECK (artwork_preference IN ('auto','local','matched','none')),
    extract_stamp TEXT,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    UNIQUE(source_id,relative_path)
);
CREATE TABLE metadata_reviews (
    id UUID PRIMARY KEY,
    owner_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    file_key TEXT NOT NULL,
    provider TEXT NOT NULL,
    provider_id TEXT NOT NULL,
    data TEXT NOT NULL,
    artwork_ref TEXT,
    expires_at TIMESTAMPTZ NOT NULL DEFAULT now() + interval '30 minutes'
);
CREATE INDEX metadata_reviews_expiry ON metadata_reviews(expires_at);
CREATE TABLE metadata_jobs (
    id UUID PRIMARY KEY,
    source_id TEXT NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    owner_id UUID NOT NULL REFERENCES users(id),
    status TEXT NOT NULL CHECK(status IN ('running','completed','failed','interrupted')),
    total INTEGER NOT NULL DEFAULT 0,
    processed INTEGER NOT NULL DEFAULT 0,
    skipped INTEGER NOT NULL DEFAULT 0,
    warnings INTEGER NOT NULL DEFAULT 0,
    message TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX metadata_one_job_per_source ON metadata_jobs(source_id) WHERE status='running';
CREATE TABLE metadata_provider_budget (
    id INTEGER PRIMARY KEY CHECK(id=1),
    next_request_at TIMESTAMPTZ NOT NULL
);
CREATE TABLE metadata_audit (
    id BIGSERIAL PRIMARY KEY,
    source_id TEXT NOT NULL,
    file_key TEXT NOT NULL,
    actor_id UUID REFERENCES users(id) ON DELETE SET NULL,
    action TEXT NOT NULL,
    before_data TEXT,
    after_data TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
