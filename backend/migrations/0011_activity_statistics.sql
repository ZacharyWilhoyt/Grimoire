-- Do not backfill elapsed viewing time or completed reading units from seek positions.
ALTER TABLE media_progress ADD COLUMN watched_seconds DOUBLE PRECISION CHECK(watched_seconds >= 0);
ALTER TABLE media_progress ADD COLUMN completed_units INTEGER[] NOT NULL DEFAULT '{}';
ALTER TABLE media_progress ADD COLUMN completed BOOLEAN NOT NULL DEFAULT false;
