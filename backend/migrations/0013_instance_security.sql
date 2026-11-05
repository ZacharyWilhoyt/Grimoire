CREATE TABLE instance_security (
    singleton BOOLEAN PRIMARY KEY DEFAULT true CHECK(singleton),
    ip_revision BIGINT NOT NULL DEFAULT 0 CHECK(ip_revision>=0),
    ip_policy JSONB NOT NULL DEFAULT '{"default_action":"allow","allow":[],"deny":[]}'::jsonb,
    updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO instance_security(singleton) VALUES(true);
