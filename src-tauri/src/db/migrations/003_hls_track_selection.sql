-- Migration 003: HLS audio/subtitle track selection (F-6)
--
-- Stores the user's selected audio and subtitle track URIs so the HLS
-- download engine can fetch them alongside the video variant and mux them
-- into the final MP4 via ffmpeg `-map`.
--
-- Both columns store a JSON array of URI strings (e.g. `["en.m3u8", "es.m3u8"]`).
-- NULL means "no selection" (backwards-compatible with tasks created before F-6).
-- The application serializes/deserializes via `serde_json`.

ALTER TABLE hls_tasks ADD COLUMN selected_audio_track_uris TEXT;
ALTER TABLE hls_tasks ADD COLUMN selected_subtitle_track_uris TEXT;
