# Installation Guide

## Prerequisites

### System Requirements

- Linux (tested on Arch Linux)
- Python 3.10+
- yt-dlp
- ffmpeg
- Telegram Bot Token

### Required Skills

- Basic command line usage
- Python virtual environments (recommended)
- Telegram bot creation via @BotFather

## Step-by-Step Installation

### 1. Create Project Directory

```bash
mkdir -p ~/projects
cd ~/projects
git init tg-media-bot
cd tg-media-bot
```

Or clone if using version control:

```bash
git clone <repo-url> tg-media-bot
cd tg-media-bot
```

### 2. Install System Dependencies

**Arch Linux:**
```bash
sudo pacman -S yt-dlp ffmpeg
```

**Ubuntu/Debian:**
```bash
sudo apt install yt-dlp ffmpeg
```

**Verify installation:**
```bash
yt-dlp --version
ffmpeg -version | head -1
```

### 3. Create Python Virtual Environment

```bash
python -m venv venv
source venv/bin/activate
```

### 4. Install Python Dependencies

```bash
pip install -r requirements.txt
```

### 5. Get Telegram Bot Token

1. Open Telegram
2. Search for @BotFather
3. Send `/newbot`
4. Follow prompts, get token
5. Copy token (starts with `:`)

### 6. Configure

```bash
cp .env.example .env
nano .env
```

Edit these values:

```env
BOT_TOKEN=123456789:ABCDefGhIJKlmNoPQRsTUVwxYZ
TEMP_DIR=/tmp/tg-media-bot
```

### 7. Create Temp Directory

```bash
mkdir -p /tmp/tg-media-bot
chmod 755 /tmp/tg-media-bot
```

### 8. Test Run

```bash
python main.py
```

Should see:
```
Starting tg-media-bot
Temp directory: /tmp/tg-media-bot
Max parallel downloads: 3
Bot initialized, starting polling...
```

## Running as System Service

### Create systemd Service

```bash
nano ~/.config/systemd/user/tg-media-bot.service
```

```ini
[Unit]
Description=Telegram Media Downloader Bot
After=network.target

[Service]
Type=simple
WorkingDirectory=/home/lad/projects/tg-media-bot
ExecStart=/home/lad/projects/tg-media-bot/venv/bin/python main.py
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
```

### Enable and Start

```bash
systemctl --user daemon-reload
systemctl --user enable --now tg-media-bot
```

### Check Status

```bash
systemctl --user status tg-media-bot
journalctl --user -u tg-media-bot -f
```

## Firefox Cookies Setup

For age-restricted or private content:

### 1. Install browser-cookies

```bash
pip install browser-cookies
```

### 2. Ensure Firefox is Default

```bash
# Open Firefox, sign in, enable sync if needed
```

### 3. Verify Cookie Access

```bash
yt-dlp --cookies-from-browser firefox --no-download "https://youtube.com"
```

If successful, you can download age-restricted videos.

## Testing

### Basic Test

1. Send `/start` to bot
2. Send `/help` to verify
3. Send a YouTube URL
4. Check for downloaded file

### Format Check

```bash
# Send to bot:
/formats https://youtube.com/watch?v=dQw4w9WgXcQ
```

Should show available formats.

## Uninstallation

```bash
# Stop service
systemctl --user stop tg-media-bot
systemctl --user disable tg-media-bot

# Remove files
rm -rf ~/projects/tg-media-bot

# Clean temp
rm -rf /tmp/tg-media-bot
```

## Docker Deployment (with Local Bot API Server)

Running via Docker with the Local Bot API Server removes the 50MB upload limit,
allowing uploads up to 2GB.

### 1. Get Telegram API Credentials

You need an `API_ID` and `API_HASH` from https://my.telegram.org/apps

1. Go to https://my.telegram.org and log in with your phone number
2. Click "API development tools"
3. Fill in the form:
   - **App title:** `My Bot`
   - **Short name:** `mybot`
   - **URL:** `https://example.com`
   - **Platform:** `Web`
   - **Description:** `Bot`
4. Submit and copy the `api_id` and `api_hash`

**Known issues with my.telegram.org:**
- The site shows a generic "ERROR" alert with no explanation
- If using a VPN, **disconnect it** — the site checks for IP/region mismatch
  with your phone number (especially problematic with Russian numbers + foreign VPN)
- Try from a **mobile browser** on the same network as your phone
- Having **Telegram Desktop** logged in and active can help
- Try a different browser, or clear cookies and log in again
- The site is genuinely broken sometimes — try again a few hours later
- New Telegram accounts may need to be a few days old

### 2. Configure

```bash
cp .env.example .env
nano .env
```

Fill in `BOT_TOKEN`, `TELEGRAM_API_ID`, and `TELEGRAM_API_HASH`.

### 3. Run

```bash
docker compose up -d
```

### 4. Stop

```bash
docker compose down
```

### 5. View Logs

```bash
docker compose logs -f bot
```

**Note:** Browser cookies (`USE_BROWSER_COOKIES`) are disabled in Docker since
there's no browser inside the container. For age-restricted content you would
need to mount a cookies file instead.

## Troubleshooting

See [[TROUBLESHOOTING]]
