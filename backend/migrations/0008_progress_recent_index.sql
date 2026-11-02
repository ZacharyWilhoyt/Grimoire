CREATE INDEX media_progress_recent ON media_progress(user_id, updated_at DESC, file_key)
WHERE position > 0;
