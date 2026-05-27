"""Tests for URL extraction in the message handler."""

from unittest.mock import MagicMock

import pytest

from src.bot.handlers import BotHandlers


@pytest.fixture
def handlers():
    # Bot is only used by the uploader, not by extract_urls
    return BotHandlers(bot=MagicMock())


class TestExtractUrls:
    def test_single_url(self, handlers):
        assert handlers.extract_urls("https://youtube.com/watch?v=x") == [
            "https://youtube.com/watch?v=x"
        ]

    def test_url_within_text(self, handlers):
        urls = handlers.extract_urls("check this https://x.com/a out")
        assert urls == ["https://x.com/a"]

    def test_multiple_urls(self, handlers):
        urls = handlers.extract_urls("https://a.com/1 https://b.com/2")
        assert urls == ["https://a.com/1", "https://b.com/2"]

    def test_trailing_punctuation_stripped(self, handlers):
        assert handlers.extract_urls("see https://x.com/a.") == ["https://x.com/a"]

    def test_no_urls(self, handlers):
        assert handlers.extract_urls("just some text") == []

    def test_empty(self, handlers):
        assert handlers.extract_urls("") == []

    def test_overlong_url_rejected(self, handlers):
        assert handlers.extract_urls("https://x.com/" + "a" * 3000) == []
