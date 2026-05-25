# Command Reference

Complete reference for all bot commands.

## Commands

### `/start`

Starts the bot and sends welcome message.

**Usage:**
```
/start
```

**Response:**
```
👋 Welcome to Media Downloader Bot!

Send me a URL to download media.
Use /help for all commands.
```

---

### `/help`

Shows help message with supported platforms.

**Usage:**
```
/help
```

**Response:**
```
<b>Media Downloader Bot</b>

Send me any media URL and I'll download and send it back to you.

<b>Commands:</b>

/start - Start the bot
/help - Show this help
/audio - Switch to audio-only mode (MP3)
/video - Switch to video download mode
/formats <url> - Show available formats
/cancel <task_id> - Cancel a download
/status - Show your active downloads

<b>Supported Platforms:</b>
• YouTube
• SoundCloud
• Vimeo
• TikTok
• Twitter/X
• Instagram
• Reddit
• Twitch
• And 1000+ more via yt-dlp
```

---

### `/audio`

Switch to audio-only download mode. Downloads will be converted to MP3.

**Usage:**
```
/audio
```

**Response:**
```
🎵 Audio-only mode enabled. Downloads will be converted to MP3.
```

**Notes:**
- Persists until changed with `/video`
- Best for music/podcasts

---

### `/video`

Switch to video download mode. Downloads will include video when available.

**Usage:**
```
/video
```

**Response:**
```
🎬 Video mode enabled. Downloads will include video when available.
```

**Notes:**
- Persists until changed with `/audio`
- Best for music videos, clips

---

### `/formats <url>`

Show available download formats for a URL.

**Usage:**
```
/formats https://youtube.com/watch?v=...
```

**Response:**
```
Available formats:

1. [18] video mp4 360p (6.5MB)
2. [22] video mp4 720p (12.3MB)
3. [137] video mp4 1080p (25.1MB)
4. [140] audio m4a (2.1MB)
5. [251] audio webm (1.8MB)

... and 10 more formats
```

**Notes:**
- Shows first 20 formats
- Lists format ID, type, extension, resolution, size
- Use format IDs with external yt-dlp

---

### `/cancel <task_id>`

Cancel a pending or active download.

**Usage:**
```
/cancel <task_id>
```

**Example:**
```
/cancel a1b2c3d4
```

**Response (success):**
```
✅ Task a1b2c3d4 cancelled.
```

**Response (failure):**
```
❌ Could not cancel task a1b2c3d4.
Make sure the task is yours and still active.
```

**Notes:**
- Get task ID from status message
- Can only cancel your own tasks
- Only works on active tasks

---

### `/status`

Show your active downloads and queue status.

**Usage:**
```
/status
```

**Response:**
```
📊 Your Downloads:

Total tasks: 5
Active: 2/2

  queued: 1
  downloading: 1
  completed: 3
```

---

## URL Processing

### Sending URLs

Simply send any supported URL as a message:

```
https://youtube.com/watch?v=dQw4w9WgXcQ
https://soundcloud.com/artist/track
https://vimeo.com/123456789
```

**Features:**
- Extracts URLs from text (no need for clean URL)
- Supports multiple URLs (up to 3 per message)
- Truncates URLs longer than 2048 characters

### Download Flow

1. URL sent → Queued
2. Status message shows progress
3. File uploaded to Telegram
4. Temp files cleaned up

### Status Messages

During download, you'll see:

```
⏳ Queued download...
Platform: youtube
Task ID: `a1b2c3d4`
Format: auto
```

```
📥 Downloading...
Task: `a1b2c3d4`
```

```
📤 Uploading...
rickroll.mp4
Size: 12.5MB
```

```
✅ Done!
rickroll.mp4
Size: 12.5MB
```

### Error Messages

```
❌ Download failed

Unsupported URL
```

```
❌ Upload failed.
File: video.mp4
Size: 125.0MB

Telegram Bot API has a 50MB upload limit.
Consider using a Local Bot API Server for larger files.
```

## Supported Platforms

| Platform | URL Patterns |
|----------|--------------|
| YouTube | youtube.com, youtu.be |
| SoundCloud | soundcloud.com |
| Vimeo | vimeo.com |
| TikTok | tiktok.com |
| Twitter/X | twitter.com, x.com |
| Instagram | instagram.com |
| Reddit | reddit.com |
| Twitch | twitch.tv |

Plus 1000+ more via yt-dlp. Run `/formats <url>` to check specific sites.

## Tips

1. **Batch downloading**: Send multiple URLs in one message (max 3)
2. **Audio mode**: Use `/audio` before sending music URLs
3. **Check formats**: Use `/formats` before downloading to see options
4. **Cancel stuck downloads**: Use `/cancel` with task ID
5. **Check status**: Use `/status` to see your queue
