"""Tests for the upload caption builder (source URL as non-linked text)."""

from src.services.uploader import _build_caption, _CAPTION_MAX


class TestBuildCaption:
    def test_none_when_empty(self):
        assert _build_caption("", None) is None

    def test_title_only_is_plain_escaped(self):
        out = _build_caption("Tom & Jerry <fun>", None)
        assert "<code>" not in out
        assert "&amp;" in out and "&lt;fun&gt;" in out

    def test_url_wrapped_in_code(self):
        out = _build_caption("My Track", "https://example.com/x")
        assert "<code>https://example.com/x</code>" in out
        assert "My Track" in out

    def test_url_ampersand_escaped(self):
        out = _build_caption("t", "https://x.com/a?b=1&c=2")
        # Escaped on the wire so Telegram doesn't choke / re-parse
        assert "a?b=1&amp;c=2" in out

    def test_title_special_chars_escaped(self):
        out = _build_caption("a<b>&'\"", "https://x.com")
        assert "<b>" not in out  # the literal tag must be escaped

    def test_url_only_no_title(self):
        out = _build_caption("", "https://x.com/y")
        assert out == "<code>https://x.com/y</code>"

    def test_long_title_truncated_within_limit(self):
        url = "https://example.com/" + "p" * 100
        out = _build_caption("T" * 5000, url)
        # The URL must survive intact and overall length stays bounded
        assert f"<code>{url}</code>" in out
        # Visible text budget respected (HTML entities aside, well under hard cap)
        assert len(out) <= _CAPTION_MAX + len("<code></code>")
