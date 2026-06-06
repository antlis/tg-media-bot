"""Application settings loaded from environment variables."""

import os
from dataclasses import dataclass, field
from pathlib import Path
from typing import Optional

from dotenv import load_dotenv

# Load .env file
load_dotenv()


@dataclass
class Settings:
    """Application configuration settings."""

    # Telegram
    bot_token: str = ""

    # Paths
    temp_dir: Path = Path("/tmp/tg-media-bot")

    # Limits
    max_parallel_downloads: int = 3
    download_timeout: int = 3600  # seconds

    # Logging
    log_level: str = "INFO"
    # Optional path to persist logs to a file (in addition to stdout). When set,
    # a rotating file handler keeps a durable record of downloads across restarts.
    log_file: str = ""

    # Rate limiting
    rate_limit_per_user: int = 2  # concurrent downloads per user

    # Local Bot API Server (for uploads >50MB)
    api_server_url: str = ""

    # Access control: empty set = open to everyone
    allowed_users: set[int] = field(default_factory=set)

    # yt-dlp options
    use_browser_cookies: bool = True
    browser_name: str = "firefox"

    # Optional proxy for yt-dlp. Only used as a fallback when a download fails
    # with a geo/region block (e.g. media that's licensed only in some regions).
    # Format: socks5h://user:pass@host:port  (or http://...)
    proxy_url: Optional[str] = None

    def __post_init__(self):
        """Validate and convert settings after initialization."""
        # Bot token is required
        if not self.bot_token:
            raise ValueError("BOT_TOKEN environment variable is required")

        # Ensure temp_dir exists
        self.temp_dir = Path(self.temp_dir)
        self.temp_dir.mkdir(parents=True, exist_ok=True)

        # Normalize log level
        self.log_level = self.log_level.upper()

    @property
    def upload_limit_mb(self) -> int:
        """Max upload size Telegram will accept, in MB.

        The standard Bot API caps uploads at 50MB; a local Bot API server
        (configured via ``api_server_url``) raises this to 2000MB.
        """
        return 2000 if self.api_server_url else 50


_settings: Optional[Settings] = None


def get_settings() -> Settings:
    """Get cached settings instance."""
    global _settings
    if _settings is None:
        _settings = _load_settings()
    return _settings


def _parse_user_ids(raw: str) -> set[int]:
    """Parse a comma-separated list of Telegram user IDs into a set of ints."""
    ids: set[int] = set()
    for part in raw.split(","):
        part = part.strip()
        if part:
            ids.add(int(part))
    return ids


def _load_settings() -> Settings:
    """Load settings from environment variables."""
    return Settings(
        bot_token=os.getenv("BOT_TOKEN", ""),
        temp_dir=Path(os.getenv("TEMP_DIR", "/tmp/tg-media-bot")),
        api_server_url=os.getenv("API_SERVER_URL", ""),
        allowed_users=_parse_user_ids(os.getenv("ALLOWED_USERS", "")),
        max_parallel_downloads=int(os.getenv("MAX_PARALLEL_DOWNLOADS", "3")),
        download_timeout=int(os.getenv("DOWNLOAD_TIMEOUT", "3600")),
        log_level=os.getenv("LOG_LEVEL", "INFO"),
        log_file=os.getenv("LOG_FILE", ""),
        rate_limit_per_user=int(os.getenv("RATE_LIMIT_PER_USER", "2")),
        use_browser_cookies=os.getenv("USE_BROWSER_COOKIES", "true").lower() == "true",
        browser_name=os.getenv("BROWSER_NAME", "firefox"),
        proxy_url=os.getenv("PROXY_URL") or None,
    )
