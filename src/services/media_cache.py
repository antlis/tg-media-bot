"""Cache of Telegram file IDs for already-uploaded media.

Telegram lets a bot resend a previously uploaded file by its ``file_id``
instantly — no re-download, no re-upload. We key cached entries on the source
URL + requested format and persist them to JSON (``MEDIA_CACHE_FILE``) so the
cache survives restarts; with no file configured it's in-memory only.
"""

import json
from pathlib import Path
from typing import Optional

from ..utils.logger import get_logger

logger = get_logger()


def _key(url: str, fmt: str) -> str:
    return f"{fmt}\n{url}"


class MediaCache:
    """Maps (url, format) → a small dict describing an uploaded Telegram file."""

    def __init__(self, path: str = ""):
        self._path: Optional[Path] = Path(path) if path else None
        self._entries: dict[str, dict] = {}
        self._load()

    def _load(self) -> None:
        if not self._path or not self._path.exists():
            return
        try:
            data = json.loads(self._path.read_text(encoding="utf-8"))
            if isinstance(data, dict):
                self._entries = data
        except (OSError, json.JSONDecodeError) as e:
            logger.error(f"Could not read media cache {self._path}: {e}")

    def get(self, url: str, fmt: str) -> Optional[dict]:
        return self._entries.get(_key(url, fmt))

    def put(self, url: str, fmt: str, entry: dict) -> None:
        self._entries[_key(url, fmt)] = entry
        self._persist()

    def evict(self, url: str, fmt: str) -> None:
        """Drop an entry (e.g. when a stored file_id is no longer valid)."""
        if self._entries.pop(_key(url, fmt), None) is not None:
            self._persist()

    def _persist(self) -> None:
        if not self._path:
            return
        try:
            self._path.parent.mkdir(parents=True, exist_ok=True)
            self._path.write_text(json.dumps(self._entries), encoding="utf-8")
        except OSError as e:
            logger.error(f"Could not write media cache {self._path}: {e}")


_media_cache: Optional[MediaCache] = None


def get_media_cache() -> MediaCache:
    """Get the global media cache instance."""
    global _media_cache
    if _media_cache is None:
        from ..config import get_settings
        _media_cache = MediaCache(get_settings().cache_file)
    return _media_cache
