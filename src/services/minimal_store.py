"""Persistent per-chat toggle for minimal UI mode.

When enabled for a chat, the bot suppresses routine progress/status messages
(queued, downloading, uploading, "Done!") and the title/source-URL caption on
uploaded media — only the media itself is sent. Failures are still reported.
IDs persist to a JSON file (``MINIMAL_MODE_FILE``) so the setting survives
restarts; with no file configured the store is in-memory only.
"""

import json
from pathlib import Path
from typing import Optional

from ..utils.logger import get_logger

logger = get_logger()


class MinimalModeStore:
    """A set of chat IDs with minimal UI mode enabled, optionally file-backed."""

    def __init__(self, path: str = ""):
        self._path: Optional[Path] = Path(path) if path else None
        self._chats: set[int] = set()
        self._load()

    def _load(self) -> None:
        if not self._path or not self._path.exists():
            return
        try:
            data = json.loads(self._path.read_text(encoding="utf-8"))
            self._chats = {int(c) for c in data}
        except (OSError, json.JSONDecodeError, TypeError, ValueError) as e:
            logger.error(f"Could not read minimal-mode file {self._path}: {e}")

    def contains(self, chat_id: int) -> bool:
        return chat_id in self._chats

    def set(self, chat_id: int, enabled: bool) -> None:
        """Enable/disable minimal mode for a chat and persist."""
        if enabled == (chat_id in self._chats):
            return
        if enabled:
            self._chats.add(chat_id)
        else:
            self._chats.discard(chat_id)
        self._persist()
        logger.info("Minimal UI mode changed", chat_id=chat_id, enabled=enabled)

    def _persist(self) -> None:
        if not self._path:
            return
        try:
            self._path.parent.mkdir(parents=True, exist_ok=True)
            self._path.write_text(json.dumps(sorted(self._chats)), encoding="utf-8")
        except OSError as e:
            logger.error(f"Could not write minimal-mode file {self._path}: {e}")


_minimal_store: Optional[MinimalModeStore] = None


def get_minimal_store() -> MinimalModeStore:
    """Get the global minimal-mode store instance."""
    global _minimal_store
    if _minimal_store is None:
        from ..config import get_settings
        _minimal_store = MinimalModeStore(get_settings().minimal_mode_file)
    return _minimal_store
