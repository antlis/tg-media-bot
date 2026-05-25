# Troubleshooting

Common issues and solutions for tg-media-bot.

## Installation Issues

### "BOT_TOKEN environment variable is required"

**Problem:** Bot doesn't start, shows error about missing token.

**Solution:**
1. Copy `.env.example` to `.env`
2. Add your Telegram bot token:
   ```env
   BOT_TOKEN=123456789:ABCDef...
   ```
3. Restart the bot

### "yt-dlp not found"

**Problem:** yt-dlp not installed.

**Solution:**
```bash
# Arch
sudo pacman -S yt-dlp

# Or via pip
pip install yt-dlp

# Verify
yt-dlp --version
```

### "ffmpeg not found"

**Problem:** ffmpeg not installed.

**Solution:**
```bash
# Arch
sudo pacman -S ffmpeg

# Ubuntu
sudo apt install ffmpeg

# Verify
ffmpeg -version | head -1
```

## Download Issues

### "Unsupported URL"

**Problem:** URL not recognized or supported.

**Solutions:**
1. Ensure URL is complete (https://...)
2. Check if platform is supported (send `/help`)
3. Try `/formats <url>` to test URL validity
4. Some platforms require authentication

### "Download failed"

**Problem:** Generic download failure.

**Solutions:**
1. Check URL is still valid
2. Video may be region-locked
3. Video may be removed/deleted
4. Network issues - try again later

### "Rate limit reached"

**Problem:** Too many concurrent downloads.

**Solution:**
- Wait for current downloads to finish
- Use `/status` to see active downloads
- Cancel unused tasks with `/cancel <task_id>`

### "File too large" / Upload failed

**Problem:** File exceeds Telegram Bot API upload limit (50MB).

**Solutions:**
- Use the Local Bot API Server (see INSTALLATION.md) for uploads up to 2GB
- Try `/audio` mode for smaller files

### "Could not fetch formats"

**Problem:** Cannot get video information.

**Solutions:**
1. Check URL is valid
2. Video may be unavailable
3. Try using Firefox cookies (see Firefox Cookies Setup)
4. Check network connectivity

## Authentication Issues

### Age-restricted content fails

**Problem:** Cannot download age-restricted videos.

**Solution - Enable Firefox Cookies:**
1. Install browser-cookies: `pip install browser-cookies`
2. Ensure Firefox is default browser
3. Sign in to Google in Firefox
4. Set in `.env`:
   ```env
   USE_BROWSER_COOKIES=true
   BROWSER_NAME=firefox
   ```
5. Restart bot

### Verify cookie access:
```bash
yt-dlp --cookies-from-browser firefox --no-download "https://youtube.com"
```

## Upload Issues

### "Upload failed"

**Problem:** Cannot send file to Telegram.

**Solutions:**
1. File may be corrupted
2. Check Telegram file size limits
3. Try with smaller file
4. Check bot has send permissions

### Bot doesn't respond

**Problem:** Bot doesn't reply to messages.

**Solutions:**
1. Start bot: `/start`
2. Check bot is running: `systemctl --user status tg-media-bot`
3. Check logs: `journalctl --user -u tg-media-bot`
4. Verify BOT_TOKEN is correct
5. Bot may be blocked by user

## Performance Issues

### Slow downloads

**Causes:**
- Server throttling (YouTube)
- Slow network
- Large file size

**Solutions:**
1. Use `/audio` for faster downloads
2. Try at off-peak hours
3. Check system resources

### High memory usage

**Solution:**
```env
MAX_PARALLEL_DOWNLOADS=1
RATE_LIMIT_PER_USER=1
```

### Temp directory full

**Solution:**
```bash
# Clean temp directory
rm -rf /tmp/tg-media-bot/*

# Or increase cleanup frequency
# Edit cleanup settings in code
```

## Logging & Debugging

### Enable debug logging

Edit `.env`:
```env
LOG_LEVEL=DEBUG
```

### View logs

```bash
# Systemd service
journalctl --user -u tg-media-bot -f

# Direct run
python main.py
```

### Common log entries

```
INFO | Download started | user_id=123 url=... platform=youtube
INFO | Download completed | size_mb=12.5 duration_sec=45.2
ERROR | Download failed | error=...
INFO | Cleanup completed | files_removed=5
WARNING | Rate limit reached | user_id=123
```

## Systemd Service Issues

### Service won't start

```bash
# Check status
systemctl --user status tg-media-bot

# View full logs
journalctl --user -u tg-media-bot -xe

# Reload after changes
systemctl --user daemon-reload
systemctl --user restart tg-media-bot
```

### Service starts but bot doesn't respond

1. Check temp directory permissions
2. Verify .env file exists and has BOT_TOKEN
3. Check yt-dlp and ffmpeg are in PATH

## Network Issues

### Firewall blocking

Ensure outbound:
- Port 443 (HTTPS)
- Telegram API: api.telegram.org

### Proxy required

```bash
# Set proxy environment
export HTTP_PROXY=http://proxy:8080
export HTTPS_PROXY=http://proxy:8080
python main.py
```

## Getting Help

If issues persist:

1. Check [[ARCHITECTURE|Architecture]] for understanding
2. Enable DEBUG logging
3. Check yt-dlp supported sites: `yt-dlp --list-extractors`
4. Test URL directly: `yt-dlp <url>`
5. Check Telegram bot @BotFather settings

## Common Error Messages

| Error | Cause | Solution |
|--------|-------|----------|
| "Invalid URL" | Malformed URL | Ensure starts with http:// or https:// |
| "Rate limit reached" | Too many downloads | Wait or cancel tasks |
| "File too large" | Exceeds 50MB Bot API limit | Use Local Bot API Server or audio mode |
| "Download failed" | Various | Check URL, network |
| "Upload failed" | Telegram issue | Retry later |
| "yt-dlp not found" | Not installed | Install yt-dlp |
