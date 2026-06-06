"""Tests for the allowlist middleware that gates every message."""

from types import SimpleNamespace

import pytest

from src.bot import router


def _make_event(chat=None):
    """A minimal stand-in for an aiogram Message with from_user + answer()."""
    answers = []

    async def answer(text):
        answers.append(text)

    return SimpleNamespace(
        from_user=SimpleNamespace(id=0),
        chat=chat,
        answer=answer,
        _answers=answers,
    )


def _patch_allowed(monkeypatch, allowed, store=None):
    monkeypatch.setattr(router, "get_settings",
                        lambda: SimpleNamespace(allowed_users=allowed))
    if store is not None:
        from src.services import chat_store
        monkeypatch.setattr(chat_store, "get_chat_store", lambda: store)


async def _run(event):
    called = {"hit": False}

    async def handler(ev, data):
        called["hit"] = True
        return "handled"

    result = await router.auth_middleware(handler, event, {})
    return called["hit"], result


class TestAuthMiddleware:
    async def test_empty_allowlist_lets_everyone(self, monkeypatch):
        _patch_allowed(monkeypatch, set())
        event = _make_event()
        event.from_user.id = 999
        hit, _ = await _run(event)
        assert hit is True

    async def test_allowed_user_passes(self, monkeypatch):
        _patch_allowed(monkeypatch, {418870313})
        event = _make_event()
        event.from_user.id = 418870313
        hit, _ = await _run(event)
        assert hit is True

    async def test_blocked_user_dropped_and_notified(self, monkeypatch):
        _patch_allowed(monkeypatch, {418870313})
        event = _make_event()
        event.from_user.id = 111
        hit, _ = await _run(event)
        assert hit is False
        assert event._answers and "not authorized" in event._answers[0].lower()

    async def test_missing_user_blocked(self, monkeypatch):
        _patch_allowed(monkeypatch, {1})
        event = _make_event()
        event.from_user = None
        hit, _ = await _run(event)
        assert hit is False


class TestGroupAllowlist:
    def _store(self):
        from src.services.chat_store import ChatStore
        return ChatStore("")  # in-memory, no file

    async def test_allowed_user_in_group_activates_it(self, monkeypatch):
        store = self._store()
        _patch_allowed(monkeypatch, {42}, store)
        event = _make_event(chat=SimpleNamespace(id=-100, type="supergroup"))
        event.from_user.id = 42
        hit, _ = await _run(event)
        assert hit is True
        assert store.contains(-100)  # remembered for other members

    async def test_other_member_passes_in_activated_group(self, monkeypatch):
        store = self._store()
        store.add(-100)  # already activated by an allowed user
        _patch_allowed(monkeypatch, {42}, store)
        event = _make_event(chat=SimpleNamespace(id=-100, type="supergroup"))
        event.from_user.id = 777  # not an allowed user
        hit, _ = await _run(event)
        assert hit is True

    async def test_stranger_in_unknown_group_blocked(self, monkeypatch):
        store = self._store()
        _patch_allowed(monkeypatch, {42}, store)
        event = _make_event(chat=SimpleNamespace(id=-555, type="supergroup"))
        event.from_user.id = 777
        hit, _ = await _run(event)
        assert hit is False

    async def test_private_chat_not_remembered(self, monkeypatch):
        store = self._store()
        _patch_allowed(monkeypatch, {42}, store)
        event = _make_event(chat=SimpleNamespace(id=42, type="private"))
        event.from_user.id = 42
        hit, _ = await _run(event)
        assert hit is True
        assert not store.contains(42)  # private chats aren't allowlisted
