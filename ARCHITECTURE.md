# Architecture

## System Overview

The bot follows a layered architecture with clear separation of concerns:

```
┌─────────────────────────────────────────┐
│           Telegram User                 │
└─────────────────┬───────────────────────┘
                  │ (Messages/Commands)
┌─────────────────▼───────────────────────┐
│         Bot Layer (aiogram)              │
│  - Command handlers                     │
│  - Message processing                    │
│  - URL extraction                        │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│         Queue Layer                       │
│  - Task management                        │
│  - Rate limiting                          │
│  - Per-user quotas                        │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│       Downloader (yt-dlp)                │
│  - URL validation                         │
│  - Platform detection                     │
│  - Media extraction                       │
└─────────────────┬───────────────────────┘
                  │
┌─────────────────▼───────────────────────┐
│        Services Layer                     │
│  - Upload to Telegram                     │
│  - File cleanup                           │
└─────────────────────────────────────────┘
```

## Components

### Bot Layer (`src/bot/`)

**handlers.py**
- Processes incoming URLs
- Extracts URLs from messages
- Creates download tasks
- Updates status messages

**router.py**
- Registers command handlers
- Sets up dispatcher
- Routes messages to appropriate handlers

### Command Layer (`src/commands/`)

Handles built-in bot commands:
- `/start` - Welcome message
- `/help` - Show help
- `/audio` - Set audio-only mode
- `/video` - Set video mode
- `/formats` - Show available formats
- `/cancel` - Cancel a task
- `/status` - Show active downloads

### Queue Layer (`src/queue/`)

**manager.py**
- Async task queue using `asyncio.Queue`
- Per-user rate limiting via semaphore
- Global concurrent download limit
- Task status tracking
- Automatic cleanup of old tasks

Key features:
- Max parallel downloads configurable
- Per-user concurrent limit
- Task cancellation support
- Status reporting

### Downloader Layer (`src/downloaders/`)

**ytdlp.py**
- Wraps yt-dlp subprocess
- Platform detection from URL patterns
- Format selection (video/audio)
- Progress tracking (future)
- Firefox cookie authentication

### Services Layer (`src/services/`)

**cleanup.py**
- Removes temporary files after upload
- Handles partial downloads
- Cleans old task directories
- Reports cleanup results

**uploader.py**
- Uploads video/audio/documents
- Handles Telegram API errors
- Respects file size limits
- Progress messages

### Config Layer (`src/config/`)

**settings.py**
- Loads from environment variables
- Validates required fields
- Creates temp directory
- Provides singleton settings instance

### Types Layer (`src/types/`)

**download.py**
- `DownloadTask` dataclass
- `DownloadStatus` enum
- `MediaFormat` enum

### Utils Layer (`src/utils/`)

**logger.py**
- Structured logging
- Log levels
- Download event logging
- Error tracking

**sanitizer.py**
- Filename sanitization
- Path traversal prevention
- Safe temp directory creation

## Data Flow

```
1. User sends URL
   ↓
2. handlers.py extracts URL
   ↓
3. Queue.add_task() - checks rate limits, creates task
   ↓
4. Background task created with asyncio.create_task()
   ↓
5. Download via ytdlp.download()
   ↓
6. Upload via uploader.upload_media()
   ↓
7. Cleanup via cleanup.cleanup_task_dir()
   ↓
8. Status message updated
```

## Security Measures

1. **No shell=True**: All subprocess calls use argument arrays
2. **URL validation**: Length check, scheme check
3. **Filename sanitization**: Removes path separators, dangerous chars
4. **Path traversal prevention**: relative_to() check
5. **Rate limiting**: Per-user concurrent download limits
6. **File size limits**: Configurable max file size

## Error Handling

- Download failures → user notification
- Upload failures → user notification with reason
- Cleanup failures → logged, doesn't block user
- Queue full → user informed, can retry
- Rate limited → user informed

## Extensibility

To add new features:

1. **New command**: Add to `commands/handlers.py`
2. **New platform**: yt-dlp handles automatically
3. **New service**: Add to `services/`
4. **New download option**: Extend `downloaders/ytdlp.py`
