-- Clean zero-state onboarding preferences and versioned idempotent curriculum metadata

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS onboarding_completed BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS preferred_study_path TEXT NOT NULL DEFAULT 'JLPT Core',
    ADD COLUMN IF NOT EXISTS placement_jlpt TEXT NOT NULL DEFAULT 'N5',
    ADD COLUMN IF NOT EXISTS daily_goal INTEGER NOT NULL DEFAULT 20;

CREATE TABLE IF NOT EXISTS content_versions (
    content_key TEXT PRIMARY KEY,
    version INTEGER NOT NULL,
    checksum TEXT NOT NULL,
    item_count INTEGER NOT NULL DEFAULT 0,
    sentence_count INTEGER NOT NULL DEFAULT 0,
    applied_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

ALTER TABLE grammar_points
    ADD COLUMN IF NOT EXISTS content_version INTEGER NOT NULL DEFAULT 1;

ALTER TABLE grammar_sentences
    ADD COLUMN IF NOT EXISTS content_version INTEGER NOT NULL DEFAULT 1;
