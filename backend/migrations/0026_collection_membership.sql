CREATE TABLE catalog_collection_memberships (
    location_id uuid PRIMARY KEY REFERENCES catalog_locations(id) ON DELETE CASCADE,
    representation_id uuid NOT NULL REFERENCES catalog_representations(id),
    target_location_id uuid NOT NULL REFERENCES catalog_locations(id) ON DELETE CASCADE,
    target_representation_id uuid NOT NULL REFERENCES catalog_representations(id),
    target_group text NOT NULL CHECK (target_group ~ '^[0-9a-f]{64}$'),
    label text NOT NULL,
    CHECK(location_id<>target_location_id)
);
CREATE INDEX catalog_collection_targets ON catalog_collection_memberships(target_group,target_location_id);
