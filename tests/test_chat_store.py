"""Tests for the persistent group-chat allowlist store."""

import json

from src.services.chat_store import ChatStore


class TestInMemory:
    def test_add_and_contains(self):
        s = ChatStore("")  # no file
        assert not s.contains(-100)
        s.add(-100)
        assert s.contains(-100)

    def test_add_is_idempotent(self):
        s = ChatStore("")
        s.add(-100)
        s.add(-100)
        assert s.contains(-100)


class TestPersistence:
    def test_persists_to_file(self, tmp_path):
        path = tmp_path / "sub" / "allowed_chats.json"  # parent created on write
        s = ChatStore(str(path))
        s.add(-100)
        s.add(-200)
        assert json.loads(path.read_text()) == [-200, -100]

    def test_loads_existing_file(self, tmp_path):
        path = tmp_path / "allowed_chats.json"
        path.write_text(json.dumps([-1, -2]))
        s = ChatStore(str(path))
        assert s.contains(-1) and s.contains(-2)

    def test_corrupt_file_is_ignored(self, tmp_path):
        path = tmp_path / "allowed_chats.json"
        path.write_text("{not json")
        s = ChatStore(str(path))  # must not raise
        assert not s.contains(-1)
