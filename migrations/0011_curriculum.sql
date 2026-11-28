-- Curriculum structure: ordered units and per-point teaching metadata.
-- Additive only; does not rename existing grammar slugs or drop columns.

CREATE TABLE IF NOT EXISTS units (
    id TEXT PRIMARY KEY,
    n INTEGER NOT NULL UNIQUE,
    level TEXT NOT NULL,
    title TEXT NOT NULL,
    goal TEXT NOT NULL,
    sort_order INTEGER NOT NULL
);

ALTER TABLE grammar_points
    ADD COLUMN IF NOT EXISTS unit_id TEXT NULL REFERENCES units(id),
    ADD COLUMN IF NOT EXISTS sort_order INTEGER NULL,
    ADD COLUMN IF NOT EXISTS prerequisites TEXT[] NOT NULL DEFAULT '{}',
    ADD COLUMN IF NOT EXISTS kind TEXT NOT NULL DEFAULT 'point';

-- Kind is 'point' or 'contrast' (seeded existing rows stay 'point').
DO $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pg_constraint WHERE conname = 'grammar_points_kind_check'
    ) THEN
        ALTER TABLE grammar_points
            ADD CONSTRAINT grammar_points_kind_check
            CHECK (kind IN ('point', 'contrast'));
    END IF;
END $$;

CREATE INDEX IF NOT EXISTS idx_grammar_points_sort_order
    ON grammar_points (sort_order ASC NULLS LAST);

CREATE INDEX IF NOT EXISTS idx_grammar_points_unit_id
    ON grammar_points (unit_id);
