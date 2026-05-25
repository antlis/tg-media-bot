# tg-media-bot

A lightweight, self-hosted Telegram media downloader bot built with Python.

## Overview

This bot downloads media from 1000+ platforms using yt-dlp and uploads the files back to Telegram. It's designed for homelab usage with minimal resource consumption and no external dependencies beyond yt-dlp and ffmpeg.

**Key Characteristics:**
- Pure utility bot - no AI, no LLM calls
- Async architecture using aiogram
- Per-user rate limiting
- Automatic temporary file cleanup
- Firefox cookie support for authenticated downloads

## Project Structure

```
tg-media-bot/
├── main.py              # Entry point
├── requirements.txt     # Python dependencies
├── .env.example         # Configuration template
├── src/
│   ├── bot/            # Bot initialization and routing
│   │   ├── handlers.py  # Message/URL processing
│   │   └── router.py   # Dispatcher setup
│   ├── commands/       # Bot commands
│   │   └── handlers.py # Command implementations
│   ├── config/         # Settings management
│   │   └── settings.py
│   ├── downloaders/    # Media downloading
│   │   └── ytdlp.py    # yt-dlp wrapper
│   ├── queue/          # Task queue
│   │   └── manager.py # Async queue with rate limiting
│   ├── services/       # Core services
│   │   ├── cleanup.py  # File cleanup
│   │   └── uploader.py # Telegram upload
│   ├── types/          # Type definitions
│   │   └── download.py
│   └── utils/          # Utilities
│       ├── logger.py   # Structured logging
│       └── sanitizer.py # Filename sanitization
```

## Quick Start

### 1. Install Dependencies

```bash
# System packages
sudo pacman -S yt-dlp ffmpeg

# Python packages
pip install -r requirements.txt
```

### 2. Configure

```bash
cp .env.example .env
nano .env
# Set BOT_TOKEN
```

### 3. Run

```bash
python main.py
```

## Documentation

- [[ARCHITECTURE|Architecture Overview]]
- [[INSTALLATION|Installation Guide]]
- [[COMMANDS|Command Reference]]
- [[TROUBLESHOOTING|Troubleshooting]]

## Related

[[../index|Back to Projects]]
