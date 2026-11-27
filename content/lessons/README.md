# Grammar lessons

One JSON file per grammar-point slug (`{slug}.json`). Files are embedded at compile time and upserted into `grammar_points.lesson` (JSONB) on every boot seed.

Points without a file keep working with the short `nuance` / `caution` / `confusable_with` fields.

## Schema

See `src/lesson.rs` (`GrammarLesson`) for the validated shape: gist, formation table, explanation, senses, register, mistakes, contrasts, examples, dialogue, checks, related.

## Editing tips

- Use existing ruby markup: `[漢字|かんじ]` (never bracket kana/katakana alone).
- Contrasts may set `other_slug` to another curriculum slug. If *both* sides have lesson files, the link must be reciprocal.
- After edits, `cargo test --lib lesson::` validates every embedded file.
