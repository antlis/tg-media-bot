"""Shared pytest fixtures and test environment setup."""

import os
import sys
from pathlib import Path

# Make `src` importable and guarantee a token so get_settings() won't bail
sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
os.environ.setdefault("BOT_TOKEN", "123:test-token")
os.environ.setdefault("USE_BROWSER_COOKIES", "false")

import pytest


@pytest.fixture(autouse=True)
def _no_ytdlp_check(monkeypatch):
    """Skip the `yt-dlp --version` probe so tests don't need the binary."""
    from src.downloaders import ytdlp
    monkeypatch.setattr(ytdlp.YtDlpDownloader, "_check_yt_dlp", lambda self: None)


@pytest.fixture(autouse=True)
def _reset_handlers_singleton():
    """Clear the shared BotHandlers between tests for isolation."""
    from src.bot import handlers
    handlers._handlers = None
    yield
    handlers._handlers = None


@pytest.fixture(autouse=True)
def _reset_chat_store_singleton():
    """Clear the shared ChatStore between tests for isolation."""
    from src.services import chat_store
    chat_store._chat_store = None
    yield
    chat_store._chat_store = None


@pytest.fixture
def reload_settings():
    """Return a callable that reloads Settings from the current environment."""
    from src.config import settings as settings_module

    def _reload():
        settings_module._settings = None
        return settings_module.get_settings()

    yield _reload
    # Reset so other tests get a clean singleton
    settings_module._settings = None
