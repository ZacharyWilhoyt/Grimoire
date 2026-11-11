-- This projection is internal. A location contributes only for the listed viewer.
CREATE VIEW catalog_remote_discovery AS
SELECT l.id AS location_id, l.representation_id, r.item_id, l.source_id,
       i.name, viewer.id AS viewer_id,
       c.last_confirmed,
       COALESCE(n.last_seen > now() - interval '90 seconds'
            AND c.last_confirmed > now() - interval '90 seconds', false) AS online,
       (viewer.id = s.owner_id OR COALESCE(p.can_access_media AND g.can_access_media, false))
           AND COALESCE(n.last_seen > now() - interval '90 seconds'
               AND c.last_confirmed > now() - interval '90 seconds', false) AS playback_eligible
FROM catalog_locations l
JOIN catalog_representations r ON r.id=l.representation_id
JOIN remote_media_representations remote
    ON remote.source_id=l.source_id AND remote.id=l.remote_representation_id
JOIN remote_media_items i ON i.source_id=remote.source_id AND i.id=remote.item_id
JOIN sources s ON s.id=l.source_id
JOIN source_nodes n ON n.id=s.node_id AND n.revoked_at IS NULL
JOIN users owner ON owner.id=s.owner_id AND NOT owner.disabled
JOIN remote_catalog_state c ON c.source_id=s.id
JOIN LATERAL (
    SELECT s.owner_id AS id
    UNION ALL
    SELECT permission.user_id FROM source_permissions permission
    WHERE permission.source_id=s.id AND permission.can_discover AND permission.user_id<>s.owner_id
) eligible ON true
JOIN users viewer ON viewer.id=eligible.id AND NOT viewer.disabled
LEFT JOIN source_permissions p ON p.source_id=s.id AND p.user_id=viewer.id
LEFT JOIN remote_source_grants g ON g.source_id=s.id AND g.user_id=viewer.id
WHERE s.kind='remote_agent'
  AND c.published
  AND c.last_confirmed > now() - interval '5 minutes'
  AND (viewer.id=s.owner_id OR (p.can_discover AND g.can_discover));
