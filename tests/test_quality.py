"""Tests for the inline quality picker."""

from types import SimpleNamespace
from unittest.mock import AsyncMock, MagicMock

import pytest

from src.bot.handlers import BotHandlers
from src.bot.quality import QUALITY_CHOICES, is_valid_choice, quality_params
from src.downloaders.ytdlp import YtDlpDownloader
from src.types.download import MediaFormat


class TestQualityParams:
    def test_audio(self):
        assert quality_params("audio") == (MediaFormat.AUDIO, None)

    def test_best(self):
        assert quality_params("best") == (MediaFormat.VIDEO, None)

    @pytest.mark.parametrize("v,h", [("1080", 1080), ("720", 720), ("480", 480)])
    def test_capped(self, v, h):
        assert quality_params(v) == (MediaFormat.VIDEO, h)

    def test_validity(self):
        assert all(is_valid_choice(v) for _, v in QUALITY_CHOICES)
        assert not is_valid_choice("9000")


class TestVideoSelector:
    def test_uncapped(self):
        sel = YtDlpDownloader._video_selector(None)
        assert "height" not in sel and "best" in sel

    def test_capped(self):
        sel = YtDlpDownloader._video_selector(720)
        assert "height<=?720" in sel


def _make_callback(data):
    msg = SimpleNamespace(chat=SimpleNamespace(id=5), edit_text=AsyncMock())
    return SimpleNamespace(
        data=data, from_user=SimpleNamespace(id=9), message=msg, answer=AsyncMock()
    )


class TestOnQualityChoice:
    @pytest.fixture
    def handlers(self):
        h = BotHandlers(bot=MagicMock())
        h.enqueue_download = AsyncMock()
        return h

    async def test_valid_choice_enqueues(self, handlers):
        token = handlers.stash_url("https://x/v")
        cb = _make_callback(f"q:{token}:720")
        await handlers.on_quality_choice(cb)
        cb.answer.assert_awaited()  # spinner cleared
        handlers.enqueue_download.assert_awaited_once()
        kwargs = handlers.enqueue_download.await_args.kwargs
        assert kwargs["url"] == "https://x/v"
        assert kwargs["preferred_format"] == MediaFormat.VIDEO
        assert kwargs["max_height"] == 720

    async def test_token_consumed(self, handlers):
        token = handlers.stash_url("https://x/v")
        await handlers.on_quality_choice(_make_callback(f"q:{token}:audio"))
        # Second press with the same token is now expired
        cb2 = _make_callback(f"q:{token}:audio")
        await handlers.on_quality_choice(cb2)
        assert handlers.enqueue_download.await_count == 1  # only the first ran

    async def test_expired_token_alerts(self, handlers):
        cb = _make_callback("q:deadbeef:720")
        await handlers.on_quality_choice(cb)
        handlers.enqueue_download.assert_not_awaited()
        # user got an alert
        assert cb.answer.await_args.kwargs.get("show_alert") is True
