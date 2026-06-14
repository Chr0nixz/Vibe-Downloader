ALTER TABLE hls_segments ADD COLUMN init_map_uri TEXT;
ALTER TABLE hls_segments ADD COLUMN init_map_local_path TEXT;
ALTER TABLE hls_segments ADD COLUMN init_map_byte_range_start INTEGER;
ALTER TABLE hls_segments ADD COLUMN init_map_byte_range_length INTEGER;
