ALTER TABLE arr_connections
    ADD COLUMN managed BOOLEAN NOT NULL DEFAULT false,
    ADD COLUMN writable_media BOOLEAN NOT NULL DEFAULT false;
