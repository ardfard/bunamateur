-- Pre-generated Japanese sentence & reading passage audio metadata and asset cache

ALTER TABLE grammar_sentences
    ADD COLUMN IF NOT EXISTS audio_url TEXT NOT NULL DEFAULT '';

ALTER TABLE grammar_sentences
    ADD COLUMN IF NOT EXISTS audio_hash TEXT NOT NULL DEFAULT '';

ALTER TABLE reading_passages
    ADD COLUMN IF NOT EXISTS audio_url TEXT NOT NULL DEFAULT '';

ALTER TABLE reading_passages
    ADD COLUMN IF NOT EXISTS audio_hash TEXT NOT NULL DEFAULT '';

CREATE TABLE IF NOT EXISTS audio_assets (
    text_hash TEXT PRIMARY KEY,
    plain_text TEXT NOT NULL,
    provider TEXT NOT NULL,
    voice TEXT NOT NULL,
    mime_type TEXT NOT NULL,
    file_path TEXT NOT NULL DEFAULT '',
    cdn_url TEXT NOT NULL DEFAULT '',
    byte_size INTEGER NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_grammar_sentences_audio_hash
    ON grammar_sentences (audio_hash)
    WHERE audio_hash <> '';

CREATE INDEX IF NOT EXISTS idx_reading_passages_audio_hash
    ON reading_passages (audio_hash)
    WHERE audio_hash <> '';

CREATE INDEX IF NOT EXISTS idx_audio_assets_provider
    ON audio_assets (provider);
