ALTER TABLE sources ADD COLUMN transcode_encoder TEXT NOT NULL DEFAULT 'cpu'
CHECK (transcode_encoder IN ('cpu','nvidia','amd','amf','auto'));
