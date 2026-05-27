# tg-media-bot

A lightweight, self-hosted Telegram media downloader bot built with Python.

## Overview

This bot downloads media from 1000+ platforms using yt-dlp and uploads the files back to Telegram. It's designed for homelab usage with minimal resource consumption and no external dependencies beyond yt-dlp and ffmpeg.

**Key Characteristics:**
- Pure utility bot - no AI, no LLM calls
- Async architecture using aiogram 3.x
- Access control via an allowlist of Telegram user IDs
- Per-user rate limiting
- Automatic temporary file cleanup
- Uploads up to 2GB via a bundled local Telegram Bot API server (vs. 50MB on the standard API)
- Firefox cookie support for authenticated downloads (non-Docker only)

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
docker compose up -d --build
docker compose logs -f bot
```

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

See [[INSTALLATION|Installation Guide]] for systemd service setup and Firefox cookie configuration.

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
| `USE_BROWSER_COOKIES` | no | `true` | Use browser cookies (forced off in Docker) |
| `BROWSER_NAME` | no | `firefox` | Browser to read cookies from |

## Access Control

The bot is gated by `ALLOWED_USERS`. An `outer_middleware` on every message (`src/bot/router.py`) checks `from_user.id` against the allowlist **before any handler runs**:

- **Empty / unset** → open to everyone.
- **Set** → only listed IDs are served; others get a denial reply and are logged.

`ALLOWED_USERS` is read at startup. To add a user, append their ID and restart:

```bash
docker compose up -d bot   # no rebuild needed — .env is read on start
```

To find a user's numeric ID, have them message [@userinfobot](https://t.me/userinfobot).

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

See [[COMMANDS|Command Reference]] for full examples and sample responses.

## Documentation

- [[ARCHITECTURE|Architecture Overview]]
- [[INSTALLATION|Installation Guide]]
- [[COMMANDS|Command Reference]]
- [[TROUBLESHOOTING|Troubleshooting]]

## Related

[[../index|Back to Projects]]
