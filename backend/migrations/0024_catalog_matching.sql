-- Attestations apply to a particular representation, never to a mutable path.
-- The explicit edition discriminator prevents a shared work ID merging cuts.
CREATE TABLE catalog_verified_identities (
    representation_id uuid PRIMARY KEY REFERENCES catalog_representations(id),
    namespace text NOT NULL,
    entity_type text NOT NULL,
    identifier text NOT NULL,
    edition text NOT NULL,
    canonical_id uuid NOT NULL REFERENCES catalog_items(id),
    evidence_id uuid REFERENCES catalog_identity_evidence(id) ON DELETE SET NULL
);
CREATE TABLE catalog_identity_groups (
    namespace text NOT NULL,
    entity_type text NOT NULL,
    identifier text NOT NULL,
    edition text NOT NULL,
    item_id uuid NOT NULL UNIQUE REFERENCES catalog_items(id),
    PRIMARY KEY (namespace, entity_type, identifier, edition)
);

-- Append-only. No FK to the physical location so history survives deletion.
CREATE TABLE catalog_review_events (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    batch_id uuid NOT NULL,
    location_id uuid NOT NULL,
    representation_id uuid NOT NULL REFERENCES catalog_representations(id),
    target_representation_id uuid REFERENCES catalog_representations(id),
    actor_id uuid REFERENCES users(id) ON DELETE SET NULL,
    scope text NOT NULL CHECK (scope IN ('owner','curator')),
    action text NOT NULL CHECK (action IN ('link','decline','unlink','verify','undo','split','verification_conflict')),
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    previous_id bigint REFERENCES catalog_review_events(id),
    effective_link_id bigint REFERENCES catalog_review_events(id),
    suppress_verified boolean NOT NULL DEFAULT false,
    candidate_signature text,
    verified_identity_id uuid REFERENCES catalog_items(id),
    verification_evidence_id uuid REFERENCES catalog_identity_evidence(id) ON DELETE SET NULL,
    evidence_snapshot jsonb,
    created_at timestamptz NOT NULL DEFAULT now()
);
CREATE INDEX catalog_review_current ON catalog_review_events(location_id, representation_id, id DESC)
    WHERE action IN ('link','unlink','undo','split','verify');
CREATE INDEX catalog_review_batch ON catalog_review_events(batch_id, id);
CREATE INDEX catalog_review_verified_alias ON catalog_review_events(verified_identity_id,location_id,representation_id)
    WHERE verified_identity_id IS NOT NULL;
CREATE INDEX catalog_review_decline ON catalog_review_events(location_id, representation_id,
    target_representation_id, candidate_signature) WHERE action='decline';

CREATE FUNCTION catalog_match_name(name text) RETURNS text
LANGUAGE sql IMMUTABLE STRICT PARALLEL SAFE
RETURN trim(regexp_replace(lower(regexp_replace(name, '\.[a-zA-Z0-9]{1,12}$', '')),
                          '[^[:alnum:]]+', ' ', 'g'));

CREATE INDEX catalog_local_match_name ON catalog_locations
    (catalog_match_name(regexp_replace(local_relative_path, '^.*/', '')))
    WHERE local_file_key IS NOT NULL;
CREATE INDEX catalog_remote_match_name ON remote_media_items(catalog_match_name(name));

-- Every row contains only the name of its own visible location.
CREATE VIEW catalog_discoverable_locations AS
SELECT d.viewer_id, d.location_id, d.representation_id, d.item_id, d.source_id,
       d.name, d.online, d.playback_eligible
FROM catalog_remote_discovery d
UNION ALL
SELECT viewer.id, l.id, l.representation_id, r.item_id, s.id,
       regexp_replace(l.local_relative_path, '^.*/', ''), true,
       grant_row.access
FROM catalog_locations l
JOIN catalog_representations r ON r.id=l.representation_id
JOIN sources s ON s.id=l.source_id AND s.kind='local_directory'
JOIN users owner ON owner.id=s.owner_id AND NOT owner.disabled
JOIN LATERAL (
    SELECT s.owner_id AS id, true AS access
    UNION ALL
    SELECT p.user_id,p.can_access_media FROM source_permissions p
    WHERE p.source_id=s.id AND p.can_discover AND p.user_id<>s.owner_id
) grant_row ON true
JOIN users viewer ON viewer.id=grant_row.id AND NOT viewer.disabled
WHERE l.local_file_key IS NOT NULL;

CREATE VIEW catalog_matching_current AS
SELECT DISTINCT ON (location_id,representation_id)
       id, location_id, representation_id, scope, action, suppress_verified, target_representation_id,
       CASE WHEN action='link' THEN id ELSE effective_link_id END AS active_link
FROM catalog_review_events
WHERE action IN ('link','unlink','undo','split','verify')
ORDER BY location_id, representation_id, id DESC;

CREATE VIEW catalog_matched_locations AS
SELECT d.*, COALESCE(
    CASE WHEN link.id IS NOT NULL AND target.id IS NOT NULL
              AND (target_identity.canonical_id IS NULL OR own_identity.canonical_id IS NULL
                   OR target_identity.canonical_id=own_identity.canonical_id)
         THEN COALESCE(target_identity.canonical_id,target.item_id) END,
    CASE WHEN NOT COALESCE(current.suppress_verified,false) THEN own_identity.canonical_id END,
    d.item_id) AS logical_id
FROM catalog_discoverable_locations d
LEFT JOIN catalog_verified_identities own_identity ON own_identity.representation_id=d.representation_id
LEFT JOIN LATERAL (
    SELECT * FROM catalog_matching_current c
    WHERE c.location_id=d.location_id AND c.representation_id=d.representation_id
) current ON true
LEFT JOIN catalog_review_events link ON link.id=current.active_link
LEFT JOIN catalog_representations target ON target.id=link.target_representation_id
    AND EXISTS(SELECT 1 FROM catalog_discoverable_locations v
               WHERE v.viewer_id=d.viewer_id AND v.representation_id=target.id)
LEFT JOIN catalog_verified_identities target_identity ON target_identity.representation_id=target.id
    AND NOT EXISTS (
        SELECT 1 FROM catalog_matching_current target_state
        JOIN catalog_locations t ON t.id=target_state.location_id AND t.representation_id=target_state.representation_id
        WHERE t.representation_id=target.id AND target_state.suppress_verified
    );

CREATE TABLE catalog_review_suppressions (
    source_id text PRIMARY KEY REFERENCES sources(id) ON DELETE CASCADE
);
CREATE TABLE catalog_review_cases (
    id uuid PRIMARY KEY,
    source_id text NOT NULL REFERENCES sources(id) ON DELETE CASCADE,
    location_id uuid NOT NULL,
    representation_id uuid NOT NULL REFERENCES catalog_representations(id),
    reporter_id uuid REFERENCES users(id) ON DELETE SET NULL,
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    created_at timestamptz NOT NULL DEFAULT now(),
    resolved_at timestamptz,
    UNIQUE(location_id,representation_id)
);
CREATE INDEX catalog_review_open ON catalog_review_cases(source_id,id) WHERE resolved_at IS NULL;
CREATE TABLE catalog_curator_audit (
    id bigint GENERATED ALWAYS AS IDENTITY PRIMARY KEY,
    actor_id uuid REFERENCES users(id) ON DELETE SET NULL,
    source_id text NOT NULL,
    case_id uuid,
    action text NOT NULL CHECK (action IN ('resolve','suppress','unsuppress')),
    reason text NOT NULL CHECK (char_length(reason) BETWEEN 1 AND 500),
    created_at timestamptz NOT NULL DEFAULT now()
);

ALTER TABLE catalog_locations ADD COLUMN local_modified_at double precision;
CREATE INDEX catalog_representation_hash ON catalog_representations(content_sha256)
    WHERE content_sha256 IS NOT NULL;
CREATE TABLE catalog_hash_jobs (
    representation_id uuid PRIMARY KEY REFERENCES catalog_representations(id),
    id uuid NOT NULL,
    state text NOT NULL CHECK(state IN ('running','ready','failed')),
    started_at timestamptz NOT NULL DEFAULT now()
);
