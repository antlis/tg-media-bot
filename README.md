# tg-media-bot

[![CI](https://github.com/antlis/tg-media-bot/actions/workflows/ci.yml/badge.svg)](https://github.com/antlis/tg-media-bot/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)
[![Python 3.11+](https://img.shields.io/badge/python-3.11%2B-blue.svg)](https://www.python.org/)

A lightweight, self-hosted Telegram media downloader bot built with Python.

**🌐 [Website & overview](https://antlis.is-a.dev/tg-media-bot/)**

## Overview

This bot downloads media from 1000+ platforms using yt-dlp and uploads the files back to Telegram. It's designed for homelab usage with minimal resource consumption and no external dependencies beyond yt-dlp and ffmpeg.

**Key Characteristics:**
- Pure utility bot - no AI, no LLM calls
- Async architecture using aiogram 3.x
- Access control via an allowlist of Telegram user IDs
- Per-user rate limiting
- Automatic temporary file cleanup
- Uploads up to 2GB via a bundled local Telegram Bot API server (vs. 50MB on the standard API)
- Audio-only sources (e.g. SoundCloud) are auto-detected and always fetched as tagged MP3
- Direct media URLs (e.g. an imageboard `.webm`) are transcoded to a streamable MP4 (H.264/AAC, `moov` at the start) so Telegram plays them inline instead of attaching as a file
- Audio results are a single post: MP3 with embedded cover art, album-art thumbnail, and title/artist/duration
- Each result post shows the original source URL as plain (non-linked) text
- Authenticated downloads via browser cookies (bare Python) or a mounted `cookies.txt` (Docker)
- Unit-tested with pytest

## Project Structure

```
tg-media-bot/
├── main.py              # Entry point: builds Bot/Dispatcher, starts polling
├── docker-compose.yml   # Bot + local Telegram Bot API server
├── Dockerfile           # Bot image (installs ffmpeg + yt-dlp)
├── requirements.txt     # Python dependencies
├── .env.example         # Configuration template
├── src/
│   ├── bot/
│   │   ├── handlers.py  # URL extraction, download/upload orchestration
│   │   └── router.py    # Dispatcher, command routing, auth middleware
│   ├── commands/
│   │   └── handlers.py  # /start, /help, /audio, /video, /status, etc.
│   ├── config/
│   │   └── settings.py  # Env-based settings (singleton)
│   ├── downloaders/
│   │   └── ytdlp.py     # yt-dlp subprocess wrapper
│   ├── queue/
│   │   └── manager.py   # Async queue + per-user rate limiting
│   ├── services/
│   │   ├── cleanup.py   # Temp file cleanup
│   │   └── uploader.py  # Telegram upload (video/audio/document)
│   ├── types/
│   │   └── download.py  # DownloadTask, DownloadStatus, MediaFormat
│   └── utils/
│       ├── logger.py    # Structured logging
│       └── sanitizer.py # Filename sanitization
└── tests/               # pytest suite (see Testing)
```

## Quick Start (Docker — recommended)

Running via Docker Compose brings up the bot **and** a local Telegram Bot API server, which raises the upload limit from 50MB to 2GB.

### 1. Get Telegram credentials

- **Bot token** from [@BotFather](https://t.me/BotFather) (`/newbot`).
- **API ID + API hash** from [my.telegram.org/apps](https://my.telegram.org/apps) (needed by the local Bot API server).

### 2. Configure

```bash
cp .env.example .env
nano .env
```

Set at minimum `BOT_TOKEN`, `TELEGRAM_API_ID`, `TELEGRAM_API_HASH`, and `ALLOWED_USERS`.

### 3. Run

```bash
docker compose up -d         # pulls the prebuilt image from GHCR
docker compose logs -f bot
```

`docker-compose.yml` references the published image `ghcr.io/antlis/tg-media-bot:latest`, so no local build is needed. To build from source instead (e.g. for unreleased changes), use `docker compose up -d --build`.

To stop: `docker compose down`.

> **Port note:** the local Bot API server publishes on host port `8082` (`8082:8081` in `docker-compose.yml`). The bot reaches it over the internal Compose network as `http://telegram-bot-api:8081`, so the host port only matters if another service already occupies `8081`. Adjust if `8082` is also taken.

## Quick Start (bare Python)

This path uses the **standard** Telegram Bot API (50MB upload limit) and supports Firefox cookies.

```bash
# System packages (Arch)
sudo pacman -S yt-dlp ffmpeg

# Python deps
python -m venv venv && source venv/bin/activate
pip install -r requirements.txt

# Configure
cp .env.example .env   # set BOT_TOKEN, ALLOWED_USERS

# Run
python main.py
```

See [Installation Guide](INSTALLATION.md) for systemd service setup and Firefox cookie configuration.

## Install on Arch (AUR)

Arch users can install a packaged build with a systemd service:

```bash
paru -S tg-media-bot          # or: yay -S tg-media-bot
sudoedit /etc/tg-media-bot/.env   # set BOT_TOKEN, ALLOWED_USERS
sudo systemctl enable --now tg-media-bot
```

Packaging files and publishing notes live in [`packaging/aur/`](packaging/aur/).

## Configuration

All settings are loaded from `.env` (see `src/config/settings.py`).

| Variable | Required | Default | Purpose |
|----------|----------|---------|---------|
| `BOT_TOKEN` | yes | — | Telegram bot token from @BotFather |
| `TELEGRAM_API_ID` | Docker only | — | From my.telegram.org/apps; used by the local Bot API server |
| `TELEGRAM_API_HASH` | Docker only | — | From my.telegram.org/apps; used by the local Bot API server |
| `ALLOWED_USERS` | recommended | empty (open) | Comma-separated Telegram user IDs allowed to use the bot |
| `API_SERVER_URL` | no | empty | Local Bot API base URL; set automatically in Docker |
| `TEMP_DIR` | no | `/tmp/tg-media-bot` | Working directory for downloads |
| `MAX_PARALLEL_DOWNLOADS` | no | `3` | Global concurrent download limit |
| `RATE_LIMIT_PER_USER` | no | `2` | Concurrent downloads per user |
| `DOWNLOAD_TIMEOUT` | no | `3600` | Per-download timeout in seconds |
| `LOG_LEVEL` | no | `INFO` | `DEBUG`/`INFO`/`WARNING`/`ERROR` |
| `LOG_FILE` | no | empty (`/data/...` in Docker) | Persist logs to a rotating file for a durable download record |
| `USE_BROWSER_COOKIES` | no | `true` | Use browser cookies (forced off in Docker) |
| `BROWSER_NAME` | no | `firefox` | Browser to read cookies from |
| `COOKIES_FILE` | no | empty | Path to a Netscape `cookies.txt` for authenticated downloads; takes precedence over browser cookies when present (the Docker way to auth) |
| `ALLOWED_CHATS_FILE` | no | empty | Path to a JSON file persisting group chats an allowed user has activated the bot in |
| `PROXY_URL` | no | empty | Proxy used **only** as a fallback retry when a download fails with a geo/region block (`socks5h://…` or `http://…`) |
| `BOT_API_HOST_PORT` | no | `8082` | Docker only: host port for the local Bot API server |

## Access Control

The bot is gated by `ALLOWED_USERS`. An `outer_middleware` on every message (`src/bot/router.py`) checks `from_user.id` against the allowlist **before any handler runs**:

- **Empty / unset** → open to everyone.
- **Set** → only listed IDs are served; others get a denial reply and are logged.

`ALLOWED_USERS` is read at startup. To add a user, append their ID and restart:

```bash
docker compose up -d bot   # no rebuild needed — .env is read on start
```

To find a user's numeric ID, have them message [@userinfobot](https://t.me/userinfobot).

### Use in groups

The bot also works in group chats. When an allowed user uses it inside a group, that group is **activated** — its other members can then use the bot there too, without being individually allowlisted. Set `ALLOWED_CHATS_FILE` to persist activated groups across restarts (in Docker this defaults to `/data/allowed_chats.json` on the `bot-logs` volume); leave it unset to keep them in memory only.

### Authenticated downloads (cookies)

Some sources (Instagram, age-restricted videos, etc.) need a logged-in session. Two options:

- **Bare Python:** set `USE_BROWSER_COOKIES=true` and `BROWSER_NAME` to pull cookies from your local browser.
- **Docker:** export a Netscape `cookies.txt`, drop it in `./cookies/`, and it's used per-download (`COOKIES_FILE=/cookies/cookies.txt`, mounted by `docker-compose.yml`). A present `cookies.txt` takes precedence over browser cookies.

## Logs & Download History

By default the bot logs to stdout (`docker compose logs bot`), which resets when the container is recreated. Set `LOG_FILE` to also persist logs to a rotating file. In Docker this is wired by default to `/data/tg-media-bot.log` on the named `bot-logs` volume, so the record of every download (timestamp, user ID, URL, platform, filename, size) **survives restarts and rebuilds**.

```bash
# Tail the persistent log
docker compose exec bot tail -f /data/tg-media-bot.log

# Just the completed downloads
docker compose exec bot grep "Download completed" /data/tg-media-bot.log
```

Rotation keeps ~110 MB of history (10 × 10 MB files). For a privacy-minded setup, set `LOG_LEVEL=WARNING` to stop recording URLs/user IDs.

## Bot Commands

Send the bot any media **URL** (or up to 3 URLs in one message) and it downloads and returns the file. The active format mode (video/audio) applies to each download.

| Command | Description |
|---------|-------------|
| `/start` | Welcome message |
| `/help` | Show help and the list of supported platforms |
| `/audio` | Switch to audio-only mode — downloads are converted to MP3 |
| `/video` | Switch to video mode (default) — includes video when available |
| `/formats <url>` | List the available download formats for a URL |
| `/status` | Show your queued/active downloads and their task IDs |
| `/cancel <task_id>` | Cancel one of your active downloads (get the ID from `/status`) |

Notes:
- `/audio` and `/video` set a **per-user** preference that persists until changed.
- A download is queued per URL; `/status` reports each one's task ID, which `/cancel` consumes.
- Per-user concurrency is bounded by `RATE_LIMIT_PER_USER`; the global cap is `MAX_PARALLEL_DOWNLOADS`.
- **Audio posts** are a single message: the MP3 with embedded cover art, an album-art thumbnail in the player, title/artist/duration tags, and the source URL in the caption. (Telegram doesn't allow a standalone photo and an audio file in one post, so the cover rides along as the player thumbnail.)
- **SoundCloud links are always audio** — no need to send `/audio` first.
- **Every post** includes the original source URL as monospace, non-linked text — copyable, but Telegram won't turn it into a link or fetch a preview.

See [Command Reference](COMMANDS.md) for full examples and sample responses.

## Testing

The test suite uses `pytest` (with `pytest-asyncio`) and covers the pure-logic units — config parsing, sanitization, URL extraction, platform detection, yt-dlp command building, the caption builder, the allowlist middleware, the queue, and ffmpeg thumbnail resizing. No network or Telegram access is required; downloads and `get_info` are mocked.

```bash
# In a virtualenv with dev deps
pip install -r requirements-dev.txt
pytest

# Or, without managing a venv (uses uv)
uv run --with pytest --with pytest-asyncio --with aiogram --with structlog \
       --with python-dotenv --with aiohttp pytest
```

The thumbnail tests need `ffmpeg` on PATH; they're skipped automatically if it's missing. yt-dlp is not required — the version probe is patched out in tests.

## Documentation

- [Architecture Overview](ARCHITECTURE.md)
- [Installation Guide](INSTALLATION.md)
- [Command Reference](COMMANDS.md)
- [Troubleshooting](TROUBLESHOOTING.md)
- [Contributor guide for AI agents](CLAUDE.md)
