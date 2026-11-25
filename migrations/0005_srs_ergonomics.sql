-- SRS ergonomics: Vacation Mode & Ghost Review intensity settings

ALTER TABLE users
    ADD COLUMN IF NOT EXISTS vacation_mode BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS vacation_started_at TIMESTAMPTZ,
    ADD COLUMN IF NOT EXISTS ghost_intensity TEXT NOT NULL DEFAULT 'standard';
