"""Persistent allowlist of group chats the bot has been activated in.

When an allowed user uses the bot inside a group, that chat is remembered so
other members of the same group can use it too. IDs persist to a JSON file
(``ALLOWED_CHATS_FILE``) so they survive restarts; with no file configured the
store is in-memory only.
"""

import json
from pathlib import Path
from typing import Optional

from ..utils.logger import get_logger

logger = get_logger()


class ChatStore:
    """A set of approved chat IDs, optionally backed by a JSON file."""

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
            logger.error(f"Could not read allowed-chats file {self._path}: {e}")

    def contains(self, chat_id: int) -> bool:
        return chat_id in self._chats

    def add(self, chat_id: int) -> None:
        """Add a chat and persist. No-op if already present."""
        if chat_id in self._chats:
            return
        self._chats.add(chat_id)
        self._persist()
        logger.info("Group chat allowed", chat_id=chat_id)

    def _persist(self) -> None:
        if not self._path:
            return
        try:
            self._path.parent.mkdir(parents=True, exist_ok=True)
            self._path.write_text(json.dumps(sorted(self._chats)), encoding="utf-8")
        except OSError as e:
            logger.error(f"Could not write allowed-chats file {self._path}: {e}")


_chat_store: Optional[ChatStore] = None


def get_chat_store() -> ChatStore:
    """Get the global chat store instance."""
    global _chat_store
    if _chat_store is None:
        from ..config import get_settings
        _chat_store = ChatStore(get_settings().allowed_chats_file)
    return _chat_store
