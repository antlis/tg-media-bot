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
    max_file_size_mb: int = 200
    max_parallel_downloads: int = 3
    download_timeout: int = 3600  # seconds

    # Logging
    log_level: str = "INFO"

    # Rate limiting
    rate_limit_per_user: int = 2  # concurrent downloads per user

    # yt-dlp options
    use_browser_cookies: bool = True
    browser_name: str = "firefox"

    def __post_init__(self):
        """Validate and convert settings after initialization."""
        # Bot token is required
        if not self.bot_token:
            raise ValueError("BOT_TOKEN environment variable is required")

        # Ensure temp_dir exists
        self.temp_dir = Path(self.temp_dir)
        self.temp_dir.mkdir(parents=True, exist_ok=True)

        # Convert max_file_size_mb to bytes
        self.max_file_size_bytes = self.max_file_size_mb * 1024 * 1024

        # Normalize log level
        self.log_level = self.log_level.upper()


_settings: Optional[Settings] = None


def get_settings() -> Settings:
    """Get cached settings instance."""
    global _settings
    if _settings is None:
        _settings = _load_settings()
    return _settings


def _load_settings() -> Settings:
    """Load settings from environment variables."""
    return Settings(
        bot_token=os.getenv("BOT_TOKEN", ""),
        temp_dir=Path(os.getenv("TEMP_DIR", "/tmp/tg-media-bot")),
        max_file_size_mb=int(os.getenv("MAX_FILE_SIZE_MB", "200")),
        max_parallel_downloads=int(os.getenv("MAX_PARALLEL_DOWNLOADS", "3")),
        download_timeout=int(os.getenv("DOWNLOAD_TIMEOUT", "3600")),
        log_level=os.getenv("LOG_LEVEL", "INFO"),
        rate_limit_per_user=int(os.getenv("RATE_LIMIT_PER_USER", "2")),
        use_browser_cookies=os.getenv("USE_BROWSER_COOKIES", "true").lower() == "true",
        browser_name=os.getenv("BROWSER_NAME", "firefox"),
    )
