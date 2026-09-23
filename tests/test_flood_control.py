"""Tests for Telegram flood-control (429) handling and status-edit fallbacks."""

from unittest.mock import AsyncMock, MagicMock

import pytest
from aiogram.exceptions import TelegramBadRequest, TelegramRetryAfter

from src.bot.handlers import BotHandlers
from src.services import uploader as uploader_mod
from src.services.uploader import UploaderService, with_flood_retry


def _retry_after(seconds=2):
    return TelegramRetryAfter(method=MagicMock(), message="Too Many Requests", retry_after=seconds)


@pytest.fixture(autouse=True)
def no_sleep(monkeypatch):
    sleep = AsyncMock()
    monkeypatch.setattr(uploader_mod.asyncio, "sleep", sleep)
    return sleep


class TestWithFloodRetry:
    async def test_retries_then_succeeds(self, no_sleep):
        send = AsyncMock(side_effect=[_retry_after(5), "ok"])
        assert await with_flood_retry(send) == "ok"
        assert send.await_count == 2
        no_sleep.assert_awaited_once_with(6)

    async def test_gives_up_after_max_retries(self):
        send = AsyncMock(side_effect=_retry_after())
        with pytest.raises(TelegramRetryAfter):
            await with_flood_retry(send)
        assert send.await_count == uploader_mod._FLOOD_MAX_RETRIES + 1

    async def test_does_not_wait_absurdly_long(self, no_sleep):
        send = AsyncMock(side_effect=_retry_after(uploader_mod._FLOOD_MAX_WAIT + 1))
        with pytest.raises(TelegramRetryAfter):
            await with_flood_retry(send)
        no_sleep.assert_not_awaited()


class TestUploadRetries:
    async def test_video_upload_survives_429(self, tmp_path):
        video = tmp_path / "v.mp4"
        video.write_bytes(b"x" * 100)
        bot = MagicMock()
        bot.send_video = AsyncMock(side_effect=[_retry_after(), MagicMock(message_id=1)])
        bot.send_document = AsyncMock()

        msg = await UploaderService(bot).upload_video(chat_id=1, file_path=video)

        assert msg is not None
        assert bot.send_video.await_count == 2
        bot.send_document.assert_not_awaited()

    async def test_persistent_429_skips_document_fallback(self, tmp_path):
        video = tmp_path / "v.mp4"
        video.write_bytes(b"x" * 100)
        bot = MagicMock()
        bot.send_video = AsyncMock(side_effect=_retry_after())
        bot.send_document = AsyncMock()

        assert await UploaderService(bot).upload_video(chat_id=1, file_path=video) is None
        bot.send_document.assert_not_awaited()


class TestStatusEdits:
    async def test_markdown_parse_error_falls_back_to_plain_text(self):
        bot = MagicMock()
        bot.edit_message_text = AsyncMock(side_effect=[
            TelegramBadRequest(method=MagicMock(), message="Bad Request: can't parse entities"),
            None,
        ])
        h = BotHandlers(bot=bot)

        await h._update_status_message(1, 2, "❌ /tmp/x/some_long_title.jpg", final=True)

        assert bot.edit_message_text.await_count == 2
        assert bot.edit_message_text.await_args.kwargs["parse_mode"] is None

    async def test_progress_edits_paused_while_flood_limited(self):
        bot = MagicMock()
        bot.edit_message_text = AsyncMock(side_effect=_retry_after(30))
        h = BotHandlers(bot=bot)

        await h._update_status_message(1, 2, "📥 10%")
        await h._update_status_message(1, 2, "📥 20%")

        # The second tick is dropped rather than hitting Telegram again.
        assert bot.edit_message_text.await_count == 1

    async def test_final_edit_waits_out_flood_limit(self):
        bot = MagicMock()
        bot.edit_message_text = AsyncMock(side_effect=[_retry_after(30), None])
        h = BotHandlers(bot=bot)
        h._edit_blocked_until[1] = float("inf")

        await h._update_status_message(1, 2, "✅ Done!", final=True)

        assert bot.edit_message_text.await_count == 2
