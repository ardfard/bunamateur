# BunAmateur (文アマ)

A **Bunpro-inspired Japanese grammar SRS & immersion companion** built with a **single-binary Rust backend** (`axum` + async `sqlx` PostgreSQL with embedded migrations + `rust-embed`), a React + TypeScript + Tailwind CSS v4 frontend, **WaniKani API v2 integration**, and design files authored in **pen.dev** (`design/bunamateur.pen`).

## Features

- **12-Stage Bunpro-Style SRS + Ghost Reviews**:
  - Cloze-deletion Japanese grammar reviews across JLPT N5–N1 (Genki, Tobira, Quartet, JLPT Core paths).
  - Missed reviews automatically spawn **Ghost Reviews** at tighter intervals until cured.
  - **Built-in Romaji → Hiragana IME** (`あ IME`) for typing answers directly without switching OS keyboards.
  - **Oops / Undo** button for genuine typos, plus alternative valid answers and confusable-grammar contrast drawers (`のに vs ても`, `ために vs ように`, `おかげで vs せいで`).
  - **Cram Mode** & **Leech Detection** for targeted drilling without disturbing SRS schedules.
- **WaniKani API v2 Integration**:
  - Connects directly via the Rust backend (`reqwest` + `rustls`) to `https://api.wanikani.com/v2` (`/user`, `/summary`, `/assignments`, `/subjects`).
  - **Smart Furigana**: Automatically hides furigana above kanji you have passed (Guru+) in WaniKani while preserving readings on unlearned kanji (hover any kanji to peek).
  - **Kanji Readiness %**: Calculates what percentage of kanji in every grammar point and reading passage you already know from WaniKani.
  - Includes a **1-click Demo Lv.14 Sync** mode to test Smart Furigana immediately without an API token.
- **Extra Learning Tools**:
  - **Conjugation Dojo (活用道場)**: High-speed verb & adjective conjugation drills (`て`, `ない`, `た`, Potential, `ば`, Passive, Causative) with streak tracking and instant grammar rules.
  - **Immersion Reading Lab (読解ラボ)**: Graded N4–N2 passages plus a custom Japanese text analyzer that detects JLPT grammar points and flags unknown kanji against your WaniKani profile.
  - **Japanese Audio TTS**: Built-in Web Speech API (`ja-JP`) playback at natural (`1.0x`) and slow (`0.7x`) speeds.
  - **Washi Light & Sumi Dark Themes** matching `design/bunamateur.pen`.

## Single-Binary Build & Deployment

```bash
# Build frontend and compile everything (embedded UI + embedded SQL migrations + pure-Rust server) into one standalone binary
make release

# Run the single binary (automatically runs embedded PostgreSQL migrations and idempotent content seeding)
DATABASE_URL=postgres://ardfard@127.0.0.1:5432/bunamateur_dev \
ENCRYPTION_KEY=$(openssl rand -hex 32) \
./target/release/bunamateur

# Optional environment variables (see .env.example)
# PORT=8080 HOST=0.0.0.0 ALLOWED_ORIGINS=https://app.example.com COOKIE_SECURE=true APP_ENV=production
```

## Security & Multi-User Auth

- **Multi-user accounts & sessions**: Argon2id password hashing, SHA-256-hashed database sessions, `HttpOnly` / `SameSite=Lax` / `Secure` session cookies, and synchronizer-token `X-CSRF-Token` validation on all state-changing routes.
- **Per-user data scoping**: `user_progress`, `review_logs`, `wk_profile`, and `wk_subjects` are isolated by `user_id`.
- **Encrypted WaniKani tokens at rest**: `wk_profile.api_token` is encrypted using AES-256-GCM via `ENCRYPTION_KEY` (64 hex chars or 32-byte base64) with per-user AAD binding.
- **Hardened HTTP layer**: Explicit `ALLOWED_ORIGINS` CORS policy (no wildcard), per-route request body limits (16 KiB auth, 64 KiB API, 256 KiB reading analysis), and per-route IP rate limiting (`auth`, `wanikani`, `general` tiers).
- **Pluggable transactional email (`EmailSender` trait)**: Ships with `ResendApiEmailSender` (`RESEND_API_KEY`, `EMAIL_FROM`, `APP_PUBLIC_URL`) and `NoopLogEmailSender` for local dev (`EMAIL_PROVIDER=log`), with `EMAIL_PROVIDER=smtp` and `SMTP_*` config reserved so an SMTP transport can be swapped in behind the same trait.

## Design File

The UI design system, light/dark variables, reusable components, and 4 screens are stored in [`design/bunamateur.pen`](design/bunamateur.pen) (authored via pen.dev).
