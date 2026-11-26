# BunAmateur (文アマ)

A **Bunpro-inspired Japanese grammar SRS & immersion companion** built with a **single-binary Rust backend** (`axum` + async `sqlx` PostgreSQL with embedded migrations + `rust-embed`), a React 19 + TypeScript + Tailwind CSS v4 frontend, **pure-Rust Japanese NLP & embedded JMdict**, **WaniKani API v2 integration**, and design files authored in **pen.dev** ([`design/bunamateur.pen`](design/bunamateur.pen)).

## Features

### 12-Stage Bunpro-Style SRS & Curriculum
- **54 JLPT N5–N1 grammar points (324+ cloze-deletion sentences)** organized across Genki, Tobira, Quartet, and JLPT Core study paths, with an interactive JLPT onboarding placement check.
- **Contextual nuance grading**: Recognizes register, conjugation, and near-miss answers with `"almost right"` shake hints, alternative valid answers, an **Oops / Undo** button for genuine typos, and confusable-grammar contrast drawers (`のに vs ても`, `ために vs ように`, `おかげで vs せいで`).
- **Flexible SRS controls**:
  - **Ghost Reviews** with configurable intensity (`minimal` / `standard` / `heavy`) that spawn tighter follow-up intervals on missed items until cured.
  - **Vacation Mode** to freeze SRS schedules while away without building a backlog.
  - **Review Wrap-Up** to finish only currently active items cleanly.
  - **Cram Mode** and **Leech Detection** for targeted drilling without disturbing SRS schedules.
- **Progress Portability**: Full **JSON / CSV progress backup and restore** (`GET /api/export/progress`, `POST /api/import/progress`).

### Pure-Rust Japanese NLP, Embedded JMdict & Personal Vocab Deck
- **Zero-external-dependency Japanese NLP (`src/nlp.rs`)**: Pure-Rust tokenizer, lemmatizer, and embedded JMdict dictionary compiled straight into the binary.
- **Immersion Reading Lab (読解ラボ)**: Graded N4–N2 reading passages plus a custom Japanese text analyzer featuring **auto-furigana**, **lemmatized JLPT grammar detection**, and **click-to-inspect JMdict definitions** with **WaniKani level badges**.
- **Personal Vocab Deck**: Save words directly while reading and export your deck with **1-click Anki `.tsv` export**.

### Study Analytics & Retention Intelligence
- **91-day review activity heatmap** tracking daily consistency.
- **JLPT N5–N1 accuracy breakdown** across levels.
- **Top leeches repair list** highlighting stubborn grammar points for focused review.

### WaniKani API v2 Integration & Conjugation Dojo
- **WaniKani Sync**: Connects directly via the Rust backend (`reqwest` + `rustls`) to `https://api.wanikani.com/v2` (`/user`, `/summary`, `/assignments`, `/subjects`) with background sync status tracking.
- **Smart Furigana & Kanji Readiness %**: Automatically hides furigana above kanji passed (Guru+) in WaniKani while preserving readings on unlearned kanji (hover any kanji to peek), and computes Kanji Readiness % across grammar points and passages.
- **1-click Demo Lv.14 Sync**: Try Smart Furigana and WaniKani level badges immediately without an API token.
- **Conjugation Dojo (活用道場)**: High-speed verb & adjective conjugation drills (`て`, `ない`, `た`, Potential, `ば`, Passive, Causative) with streak tracking and instant grammar rules.

### Frontend Polish & PWA
- **History API URL Routing**: Shareable, deep-linkable routes (`/`, `/reviews`, `/reviews?mode=cram|ghost|lesson`, `/grammar`, `/grammar/:slug`, `/conjugation`, `/reading`, `/wanikani`).
- **3-Mode Romaji IME (`Hiragana` / `Katakana` / `Off`)**: Built-in romaji conversion with mid-string cursor editing and full OS IME composition safety.
- **Review Keyboard Shortcuts**: Fast keyboard-first review flow alongside Washi Light & Sumi Dark themes matching [`design/bunamateur.pen`](design/bunamateur.pen).
- **PWA Offline Support**: Installable web app with offline asset caching.

### Pre-Generated Japanese Sentence Audio
- Serves pre-generated high-quality Japanese sentence audio via `GET /api/audio/sentence/:id` (with configuration status at `GET /api/audio/status`) from `AUDIO_CACHE_DIR` (`./data/audio`) or `AUDIO_CDN_BASE_URL`.
- Includes [`scripts/generate_audio.ts`](scripts/generate_audio.ts) for batch content-addressed synthesis using local **VOICEVOX** or **Azure Neural TTS**, skipping already-cached files and writing a `manifest.json` index.
- Seamlessly falls back to the browser **Web Speech API** (`ja-JP`, `1.0x` and `0.7x` speeds) whenever no pre-generated asset or external provider is configured.

### Multi-User Security & Authentication
- **Accounts & Sessions**: Argon2id password hashing, **Google OAuth 2.0 / OIDC** login with PKCE S256 (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`, `GOOGLE_REDIRECT_URI`) and verified-email account linking, SHA-256-hashed database sessions, `HttpOnly` / `SameSite=Lax` / `Secure` session cookies, and `X-CSRF-Token` synchronizer-token protection on state-changing routes.
- **Self-Service Password Reset & Pluggable Email (`EmailSender` trait)**: Ships with `ResendApiEmailSender` (`RESEND_API_KEY`, `EMAIL_FROM`, `APP_PUBLIC_URL`) and `NoopLogEmailSender` (`EMAIL_PROVIDER=log` for local dev), with `EMAIL_PROVIDER=smtp` and `SMTP_*` variables reserved for an SMTP transport behind the same trait.
- **Active Session Management & Revocation**: Inspect active sessions and revoke individual or other sessions across devices.
- **Encrypted Secrets & Per-User Isolation**: All user progress, logs, vocab, and WaniKani data are scoped by `user_id`; WaniKani API tokens are encrypted at rest with **AES-256-GCM** (`ENCRYPTION_KEY`) using per-user AAD binding.
- **Hardened HTTP Layer**: Explicit `ALLOWED_ORIGINS` CORS policy (no wildcard), per-route body limits (16 KiB auth, 64 KiB API, 256 KiB reading/import), and tiered per-IP rate limiting (`auth`, `wanikani`, `general`; reverse-proxy `X-Forwarded-For` opt-in via `TRUST_PROXY=true`).

### Production Operations & CI/CD
- **Observability Endpoints**:
  - `GET /healthz` — Liveness probe with service uptime.
  - `GET /readyz` — Readiness probe verifying live PostgreSQL connectivity and audio provider status.
  - `GET /metrics` — Prometheus text exposition (`0.0.4`), optionally protected by `METRICS_BEARER_TOKEN`.
- **Runtime Efficiency**: Structured JSON logging (`LOG_FORMAT=json`, `RUST_LOG`), HTTP gzip compression, `ETag` / `304 Not Modified` responses, immutable `Cache-Control` headers for fingerprinted frontend assets, and graceful `SIGTERM` / `SIGINT` shutdown.
- **Container & Release Automation**: Multi-stage [`Dockerfile`](Dockerfile) with non-root runtime user and `HEALTHCHECK`, plus GitHub Actions workflows for CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) and tagged releases ([`.github/workflows/release.yml`](.github/workflows/release.yml) publishing Linux `x86_64` tarballs + SHA-256 checksums and multi-arch `linux/amd64,linux/arm64` GHCR images).

---

## Quick Start

### Prerequisites
- **Rust** (stable toolchain, 1.92+)
- **Bun** (1.2+)
- **PostgreSQL** (16+)

### 1. Configure Environment
Copy [`.env.example`](.env.example) to `.env` and set your database URL and 32-byte encryption key:

```bash
cp -f .env.example .env
# Generate a 64-character hex ENCRYPTION_KEY:
openssl rand -hex 32
```

Key environment variables (see [`.env.example`](.env.example) for the complete list):

| Variable | Description |
| --- | --- |
| `DATABASE_URL` | PostgreSQL connection URL (embedded migrations & seed data run automatically on startup) |
| `TEST_DATABASE_URL` | PostgreSQL database URL used by Rust integration tests |
| `ENCRYPTION_KEY` | 64 hex chars or 32-byte base64 key for AES-256-GCM WaniKani token encryption (required in production) |
| `HOST` / `PORT` | Server bind address and port (defaults: `127.0.0.1` / `8080`, `0.0.0.0` in Docker) |
| `APP_ENV` / `COOKIE_SECURE` | Set `APP_ENV=production` and `COOKIE_SECURE=true` behind HTTPS |
| `ALLOWED_ORIGINS` | Comma-separated allowed CORS origins |
| `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` | Enables Google OAuth 2.0 / OIDC login (redirect URI: `<APP_PUBLIC_URL>/api/auth/google/callback`) |
| `EMAIL_PROVIDER` / `RESEND_API_KEY` | Transactional email transport (`resend`, `log`, or reserved `smtp`) |
| `LOG_FORMAT` / `METRICS_BEARER_TOKEN` | `pretty` or `json` logs; optional bearer token for `GET /metrics` |
| `AUDIO_CACHE_DIR` / `AUDIO_CDN_BASE_URL` | Local directory (`./data/audio`) or CDN base URL for pre-generated sentence audio |

### 2. Run Locally (Development)
```bash
# Build frontend + Rust debug binary and launch server on http://127.0.0.1:8080
make run
```

For frontend hot-reload alongside the running Rust backend:
```bash
cd frontend && bun run dev
```

### 3. Single-Binary Production Build
```bash
# Compiles frontend assets + SQL migrations + NLP/JMdict + Axum server into one binary
make release

# Run the standalone binary
DATABASE_URL=postgres://user:pass@127.0.0.1:5432/bunamateur \
ENCRYPTION_KEY=$(openssl rand -hex 32) \
APP_ENV=production \
LOG_FORMAT=json \
ALLOWED_ORIGINS=https://app.example.com \
./target/release/bunamateur
```

### 4. Pre-Generated Sentence Audio (Optional)
Preview what audio assets would be synthesized against a running local server (or `--input sentences.json`):

```bash
# Dry-run hash & synthesis plan (no TTS calls or writes)
make audio-dry-run

# Generate audio using local VOICEVOX (default: http://127.0.0.1:50021)
bun run scripts/generate_audio.ts --provider voicevox --out ./data/audio

# Or generate using Azure Neural TTS
AZURE_SPEECH_KEY=... AZURE_SPEECH_REGION=japaneast \
bun run scripts/generate_audio.ts --provider azure --voice ja-JP-NanamiNeural --out ./data/audio
```

---

## Testing & Quality Checks

All core checks are exposed via the [`Makefile`](Makefile):

```bash
# Frontend unit tests (Bun test runner: IME, furigana, router)
make test-frontend
# Equivalent: cd frontend && bun test

# Rust linter (fails on any warnings, matching CI)
make clippy
# Equivalent: cargo clippy --all-targets -- -D warnings

# Rust unit & PostgreSQL integration tests (requires TEST_DATABASE_URL / DATABASE_URL)
make test
# Equivalent: cargo test

# Full local verification suite (frontend tests + frontend build + clippy + cargo test)
make check
```

### Playwright End-to-End Tests
With the server running at `http://127.0.0.1:8080` (or custom `PLAYWRIGHT_BASE_URL`):

```bash
cd frontend
bunx playwright install chromium
bunx playwright test
```

---

## Docker Deployment

The multi-stage [`Dockerfile`](Dockerfile) builds the frontend with `oven/bun:1.2-alpine`, compiles the release Rust binary with `rust:1.92-bookworm`, and produces a minimal non-root `debian:bookworm-slim` runtime image with a built-in `/healthz` `HEALTHCHECK`.

```bash
# Build the container image
make docker
# Equivalent: docker build -t bunamateur:latest .

# Run with PostgreSQL and required secrets
docker run --rm -p 8080:8080 \
  -e DATABASE_URL="postgres://user:pass@host.docker.internal:5432/bunamateur" \
  -e ENCRYPTION_KEY="$(openssl rand -hex 32)" \
  -e APP_ENV="production" \
  -e COOKIE_SECURE="true" \
  -e ALLOWED_ORIGINS="https://app.example.com" \
  -e APP_PUBLIC_URL="https://app.example.com" \
  -e LOG_FORMAT="json" \
  -v "$(pwd)/data/audio:/app/data/audio:ro" \
  -e AUDIO_CACHE_DIR="/app/data/audio" \
  bunamateur:latest
```

---

## Version Control (`jj`)

This repository uses **[Jujutsu (`jj`)](https://martinvonz.github.io/jj/)** colocated with Git for version control:

```bash
jj status               # Inspect working-copy changes
jj diff                 # View current diff
jj log                  # View commit graph
jj describe -m "..."    # Set or update the current change description
jj commit -m "..."      # Finalize current change and start a new one
jj git fetch            # Fetch updates from Git remote
jj git push             # Push bookmarks to Git remote
```

If cloning fresh with `git clone` and `.jj/` is not initialized yet, run `jj git init` to enable a colocated Jujutsu workspace.

---

## Design File

The UI design system, light/dark variables, reusable components, and core screens are stored in [`design/bunamateur.pen`](design/bunamateur.pen) (authored via pen.dev).
