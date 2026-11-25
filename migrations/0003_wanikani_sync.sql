-- Hardened WaniKani v2 incremental sync cursors, background job status & scheduled auto-sync

ALTER TABLE wk_profile
    ADD COLUMN IF NOT EXISTS sync_status TEXT NOT NULL DEFAULT 'idle',
    ADD COLUMN IF NOT EXISTS sync_progress_pages INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS sync_progress_items INTEGER NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS sync_message TEXT NOT NULL DEFAULT '',
    ADD COLUMN IF NOT EXISTS sync_error TEXT,
    ADD COLUMN IF NOT EXISTS sync_started_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS user_etag TEXT,
    ADD COLUMN IF NOT EXISTS summary_etag TEXT,
    ADD COLUMN IF NOT EXISTS assignments_updated_after TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS assignments_etag TEXT,
    ADD COLUMN IF NOT EXISTS subjects_updated_after TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS subjects_etag TEXT,
    ADD COLUMN IF NOT EXISTS auto_sync_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    ADD COLUMN IF NOT EXISTS auto_sync_interval_mins INTEGER NOT NULL DEFAULT 60;

CREATE INDEX IF NOT EXISTS idx_wk_profile_auto_sync
    ON wk_profile (auto_sync_enabled, last_synced_at)
    WHERE api_token IS NOT NULL AND is_demo = FALSE;
