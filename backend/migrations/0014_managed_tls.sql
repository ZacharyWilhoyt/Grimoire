ALTER TABLE instance_security ADD COLUMN tls_revision BIGINT NOT NULL DEFAULT 0 CHECK(tls_revision>=0);
ALTER TABLE instance_security ADD COLUMN tls_settings JSONB;
CREATE TABLE tls_certificate_events (
    domain TEXT PRIMARY KEY,
    last_renewal DOUBLE PRECISION,
    last_success DOUBLE PRECISION,
    last_error DOUBLE PRECISION
);
