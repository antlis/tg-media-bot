# CLAUDE.md

Guidance for Claude Code when working in this repository.

## What this is

A self-hosted Telegram bot that downloads media via `yt-dlp` and uploads it back to the user. Pure utility — no LLM calls. Built on **aiogram 3.x** (async). Targets homelab/Docker deployment.

## Run / build / test

```bash
# Docker (primary path: bot + local Telegram Bot API server, 2GB uploads)
docker compose up -d --build       # build + start both services
docker compose logs -f bot         # follow bot logs
docker compose up -d bot           # restart bot ONLY after .env change (no rebuild)
docker compose up -d --build bot   # rebuild bot after CODE change (see gotcha below)
docker compose down                # stop

# Bare Python (standard API, 50MB limit, supports Firefox cookies)
python -m venv venv && source venv/bin/activate
pip install -r requirements.txt
python main.py

# Tests (pytest + pytest-asyncio)
pip install -r requirements-dev.txt && pytest
# …or without a venv:
uv run --with pytest --with pytest-asyncio --with aiogram --with structlog \
       --with python-dotenv --with aiohttp pytest

# Syntax check
python -m py_compile main.py src/**/*.py
```

Tests live in `tests/` and cover the pure-logic units (config parsing, sanitization, URL extraction, platform detection, yt-dlp command building, caption builder, allowlist middleware, queue, ffmpeg thumbnail resizing). They use **no network or Telegram**: `get_info`/downloads are mocked, and `tests/conftest.py` patches out the `yt-dlp --version` probe and sets a dummy `BOT_TOKEN`. `pytest.ini` sets `asyncio_mode = auto`, so `async def test_*` functions run directly. The ffmpeg-dependent thumbnail tests self-skip when ffmpeg is absent. After non-trivial changes, run pytest; for end-to-end behaviour also exercise the bot in Telegram or via `docker compose exec -T bot python -c "..."`.

## Architecture

Request flow: Telegram → `dp` (aiogram Dispatcher) → `auth_middleware` → handler → queue → `yt-dlp` → uploader → cleanup.

- `main.py` — entry point. Builds `Bot`/`Dispatcher`, wires graceful shutdown, starts polling. Chooses local-API-server session vs. standard API based on `API_SERVER_URL`.
- `src/bot/router.py` — `create_router()` builds the `Dispatcher`, registers the **auth middleware** (`outer_middleware` on `dp.message` and `dp.callback_query`), all command/text handlers, and the `q:`-prefixed callback for the inline quality picker.
- `src/bot/quality.py` — inline quality-picker choices (`QUALITY_CHOICES`) and `quality_params()` mapping each to `(MediaFormat, max_height)`. `/formats` renders these as buttons; `BotHandlers.on_quality_choice` resolves the tapped token (`stash_url`/`_pending`) and calls `enqueue_download`.
- `src/bot/handlers.py` — `BotHandlers`: URL extraction, queueing, the background download→upload→cleanup task, status-message editing.
- `src/commands/handlers.py` — `CommandHandlers`: `/start /help /audio /video /cancel /status /formats`.
- `src/config/settings.py` — `Settings` dataclass loaded once from env (`get_settings()` singleton). All config flows through here.
- `src/utils/logger.py` — `StructuredLogger`: stdout always; when `LOG_FILE` is set, also a `RotatingFileHandler` (10MB × 10) for a durable download record. Handler setup is **idempotent** — modules call `get_logger()` at import time (stdout only) before `main()` re-runs `setup_logging()` with the file path, so handlers are added if-missing rather than all-or-nothing. In Docker, `LOG_FILE=/data/tg-media-bot.log` on the `bot-logs` volume persists across rebuilds.
- `src/downloaders/ytdlp.py` — `YtDlpDownloader`: subprocess wrapper around `yt-dlp`. Platform detection, format selection. `AUDIO_ONLY_PLATFORMS` (e.g. soundcloud) are forced to audio via `_effective_format()` regardless of the user's preference. Audio downloads embed cover art + tags (`--embed-thumbnail/--embed-metadata`) and write `--write-info-json`; `_find_thumbnail()` / `_read_info_json()` surface cover art, title, artist, and duration on `DownloadResult`. Video downloads add `--recode-video mp4` + `-movflags +faststart` so direct-file URLs (e.g. `.webm`) come out as Telegram-streamable MP4. **Geo-block fallback:** if a download fails and the stderr matches `_GEO_MARKERS` (country/region licensing block) *and* `PROXY_URL` is set, it retries **once** through `--proxy` (intended for CIS-licensed media via a regional exit). The proxy is *only* used on this fallback, never on the first attempt.
- `src/types/download.py` — shared domain types used across modules: `DownloadTask`, `DownloadStatus` enum (queued→downloading→processing→uploading→completed/failed/cancelled), `MediaFormat` enum (`video`/`audio`/`auto`).
- `src/queue/manager.py` — async queue + per-user concurrency limits.
- `src/services/uploader.py` — `UploaderService`: sends video/audio/document, falls back to document for large/unknown. Audio is a **single** message — Telegram forbids combining a standalone photo with an audio file in one post/album, so the cover rides along as the audio's album-art thumbnail. `_build_caption()` appends the source URL inside `<code>` (sent with `parse_mode="HTML"`) so it shows as non-linked text. `_prepare_thumbnail()` ffmpeg-resizes cover art to Telegram's thumbnail limits (≤320px, <200KB); failure is non-fatal.
- `src/services/cleanup.py` — removes per-task temp dirs.
- `src/services/chat_store.py` — `ChatStore`: persistent allowlist of group chats an allowed user has activated the bot in. Backed by `ALLOWED_CHATS_FILE` (JSON) when set, in-memory otherwise. Singleton via `get_chat_store()`.
- `src/services/media_cache.py` — `MediaCache`: maps `(url, format)` → an uploaded file's `{kind, file_id, …}` so repeat requests are resent by `file_id` instantly (no re-download). Backed by `MEDIA_CACHE_FILE` when set. `_process_download_task` checks it before acquiring a slot; on a stale `file_id` the entry is evicted and the download proceeds. Filled from `cache_entry_from_message()` after a successful upload.
- `src/utils/sanitizer.py` — filename sanitization / path-traversal prevention.

`YtDlpDownloader._cookie_args()` centralizes cookie flags: a `COOKIES_FILE` (when the file exists) wins over `--cookies-from-browser`; used by both `get_info()` and `_build_command()`.

**Live progress:** downloads run with `--newline --progress-template "download:PROG|…"`; `_run_download` streams stdout via `_stream()`, parses `PROG|` lines (`parse_progress_line`), and invokes the `progress_callback` at most once per `_PROGRESS_MIN_INTERVAL` (always on 100%). `BotHandlers._process_download_task` passes a callback that edits the status message with `render_progress_bar`. Only the download phase has progress — the Bot API exposes no upload-progress callback.

## Conventions

- **Config only via `src/config/settings.py`.** Add a field to `Settings`, read the env var in `_load_settings()`. Don't call `os.getenv` elsewhere.
- **aiogram 3.x APIs** (not 2.x). E.g. local API server requires a `TelegramAPIServer` object, not a URL string (see gotcha).
- **No `shell=True`.** Subprocess calls use argument arrays (`yt-dlp` wrapper). Keep it that way.
- Structured logging via `src/utils/logger.py` (`get_logger()`), not bare `print`.
- Secrets live in `.env` (gitignored). Never commit tokens/keys. Redact when quoting config.

## Gotchas (learned the hard way)

- **Code changes need an image rebuild.** The Dockerfile does `COPY . .`, so editing Python and running `docker compose up -d bot` runs the OLD code. Use `--build` for code; plain restart is only enough for `.env` changes (mounted via `env_file`).
- **Local API server needs `TelegramAPIServer.from_base(url)`** passed to `AiohttpSession(api=...)`. Passing the raw URL string crashes with `'str' object has no attribute 'api_url'`.
- **`is_local=True` would break uploads here.** The bot and `telegram-bot-api` containers do NOT share a volume, so the API server can't read the bot's local file paths. Uploads must go over HTTP (`from_base` default `is_local=False`).
- **Host port 8082, not 8081.** `8081` was already taken on the deployment host (GitLab). The bot uses the internal Compose network name regardless; the host mapping is only for external access.
- **Browser cookies are off in Docker** (`USE_BROWSER_COOKIES: "false"` in compose) — no browser in the container. For authenticated downloads in Docker you'd mount a cookies file instead.
- **`telegram-bot-api` healthcheck must be a TCP probe, not HTTP.** The server returns 404 on `/` (no root route), so `wget --spider` fails and the container stays `unhealthy`, which blocks the bot via `depends_on: service_healthy`. Use `nc -z 127.0.0.1 8081` instead.

## Access control

`ALLOWED_USERS` (comma-separated Telegram user IDs) is enforced by `auth_middleware` in `src/bot/router.py`, gating every message before handlers. Empty allowlist = open to everyone. Read at startup, so adding an ID only needs a bot restart, not a rebuild.

**Groups:** access is also granted in any group chat that an allowed user has used the bot in. The first time an allowed user sends a message in a group/supergroup, `auth_middleware` records the chat in the `ChatStore` (`src/services/chat_store.py`), after which the group's other members are served too. Activated chats persist to `ALLOWED_CHATS_FILE` when set.
