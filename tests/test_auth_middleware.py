"""Tests for the allowlist middleware that gates every message."""

from types import SimpleNamespace

import pytest

from src.bot import router


def _make_event():
    """A minimal stand-in for an aiogram Message with from_user + answer()."""
    answers = []

    async def answer(text):
        answers.append(text)

    return SimpleNamespace(
        from_user=SimpleNamespace(id=0),
        answer=answer,
        _answers=answers,
    )


def _patch_allowed(monkeypatch, allowed):
    monkeypatch.setattr(router, "get_settings",
                        lambda: SimpleNamespace(allowed_users=allowed))


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
