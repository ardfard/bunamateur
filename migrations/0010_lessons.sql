-- Optional rich lesson payload per grammar point (validated JSON; NULL = short-field fallback).
ALTER TABLE grammar_points
    ADD COLUMN IF NOT EXISTS lesson JSONB;
