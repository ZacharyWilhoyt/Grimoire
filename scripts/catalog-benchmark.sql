-- Only run through benchmark-unified-catalog.sh in its disposable database.
\timing on
INSERT INTO users(id,username,password_hash) VALUES
('00000000-0000-0000-0000-000000000001','fixture_viewer','unused'),
('00000000-0000-0000-0000-000000000002','fixture_owner','unused');
INSERT INTO source_nodes(id,owner_id,last_seen) SELECT
 lpad(to_hex(i),32,'0')::uuid,'00000000-0000-0000-0000-000000000002',
 now()-CASE WHEN i=12 THEN interval '100 seconds' ELSE interval '0' END
FROM generate_series(11,13) i;
INSERT INTO sources(id,owner_id,name,kind,node_id)
SELECT 'remote:'||id,owner_id,'fixture','remote_agent',id FROM source_nodes;
INSERT INTO sources(id,owner_id,name,path) VALUES
('local','00000000-0000-0000-0000-000000000001','local','/fixture');
INSERT INTO source_permissions(source_id,user_id,can_discover,can_access_media)
SELECT id,'00000000-0000-0000-0000-000000000001',true,true
FROM sources WHERE node_id IN ('00000000-0000-0000-0000-00000000000b','00000000-0000-0000-0000-00000000000c');
INSERT INTO remote_catalog_state(source_id,revision,digest,published)
SELECT id,1,'fixture',true FROM sources WHERE node_id IS NOT NULL;
INSERT INTO remote_source_grants(source_id,user_id,can_discover,can_access_media)
SELECT id,'00000000-0000-0000-0000-000000000001',true,true FROM sources WHERE node_id IS NOT NULL;
CREATE TEMP TABLE seed AS SELECT i,lpad(to_hex(i),32,'0')::uuid AS item FROM generate_series(100,2099) i;
CREATE TEMP TABLE copies AS SELECT seed.*,s.id AS source,s.node_id,gen_random_uuid() AS rep,gen_random_uuid() AS location
FROM seed CROSS JOIN sources s;
INSERT INTO catalog_items(id,kind) SELECT item,'video' FROM seed;
INSERT INTO catalog_representations(id,item_id,extension,size_bytes) SELECT rep,item,'mp4',4096 FROM copies;
INSERT INTO remote_media_items(source_id,id,name) SELECT source,item,'Episode '||i||'.mp4' FROM copies WHERE node_id IS NOT NULL;
INSERT INTO remote_media_representations(source_id,item_id,id,extension,size_bytes)
SELECT source,item,rep,'mp4',4096 FROM copies WHERE node_id IS NOT NULL;
INSERT INTO remote_media_locations(source_id,representation_id,node_id)
SELECT source,rep,node_id FROM copies WHERE node_id IS NOT NULL;
INSERT INTO catalog_locations(id,representation_id,source_id,remote_representation_id)
SELECT location,rep,source,rep FROM copies WHERE node_id IS NOT NULL;
INSERT INTO catalog_locations(id,representation_id,source_id,local_file_key,local_relative_path)
SELECT location,rep,source,lpad(to_hex(i),64,'0'),'Episode '||i||'.mp4' FROM copies WHERE node_id IS NULL;
ANALYZE;
SELECT count(*) AS remote_locations,count(*) FILTER(WHERE playback_eligible) AS remote_playable
FROM catalog_remote_page_locations WHERE viewer_id='00000000-0000-0000-0000-000000000001';
SELECT count(*) AS first_page_rows FROM (
    SELECT DISTINCT ON(v.logical_id) v.logical_id,v.location_id
    FROM catalog_matched_locations v WHERE v.viewer_id='00000000-0000-0000-0000-000000000001'
    ORDER BY v.logical_id,v.playback_eligible DESC,v.online DESC,v.location_id LIMIT 31
) page;
SELECT count(*) FROM catalog_matched_locations
WHERE viewer_id='00000000-0000-0000-0000-000000000001'
  AND logical_id='00000000-0000-0000-0000-000000000064' AND playback_eligible;

DO $$
DECLARE visible integer; playable integer;
BEGIN
    SELECT count(*),count(*) FILTER (WHERE playback_eligible) INTO visible,playable
    FROM catalog_remote_page_locations WHERE viewer_id='00000000-0000-0000-0000-000000000001';
    IF visible<>4000 OR playable<>2000 THEN
        RAISE EXCEPTION 'Unexpected remote projection counts: %, %', visible, playable;
    END IF;
END
$$;
