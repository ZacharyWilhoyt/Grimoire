-- Metadata is the validated provider response for this representation.
-- It is projected only through the representation's own discovery grant.
ALTER TABLE catalog_verified_identities ADD COLUMN provider_fields jsonb;
ALTER TABLE catalog_verified_identities ADD COLUMN provider_art text REFERENCES metadata_artwork(id);

CREATE VIEW catalog_remote_page_locations AS
SELECT v.viewer_id,v.location_id,v.representation_id,v.item_id,v.logical_id,v.source_id,
       s.name AS source_name,v.name,v.online,v.playback_eligible,r.extension,r.size_bytes,
       l.remote_representation_id,
       s.owner_id=v.viewer_id OR COALESCE(p.can_access_media AND g.can_access_media,false) AS media_granted,
       e.namespace,e.entity_type,e.identifier,e.provider_fields,e.provider_art,
       CASE WHEN e.namespace='tmdb_tv' AND e.entity_type='episode'
            THEN encode(sha256(convert_to('tmdb:series:' || split_part(e.identifier,':',1),'UTF8')),'hex') END AS series_group
FROM catalog_matched_locations v
JOIN catalog_locations l ON l.id=v.location_id AND l.remote_representation_id IS NOT NULL
JOIN catalog_representations r ON r.id=v.representation_id
JOIN sources s ON s.id=v.source_id
JOIN remote_media_locations ml ON ml.source_id=s.id AND ml.representation_id=l.remote_representation_id
    AND ml.node_id=s.node_id
LEFT JOIN source_permissions p ON p.source_id=s.id AND p.user_id=v.viewer_id
LEFT JOIN remote_source_grants g ON g.source_id=s.id AND g.user_id=v.viewer_id
LEFT JOIN catalog_matching_current c ON c.location_id=v.location_id AND c.representation_id=v.representation_id
LEFT JOIN catalog_verified_identities e ON e.representation_id=v.representation_id
    AND NOT COALESCE(c.suppress_verified,false);
