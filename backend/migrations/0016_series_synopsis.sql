ALTER TABLE media_groups
    ADD COLUMN synopsis TEXT,
    ADD COLUMN synopsis_provider TEXT;

ALTER TABLE metadata_reviews ADD COLUMN group_id TEXT;
