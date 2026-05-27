"""Tests for filename/path sanitization."""

from pathlib import Path

from src.utils.sanitizer import (
    get_safe_temp_dir,
    sanitize_filename,
    sanitize_path,
)


class TestSanitizeFilename:
    def test_plain_name_unchanged(self):
        assert sanitize_filename("song.mp3") == "song.mp3"

    def test_empty_becomes_untitled(self):
        assert sanitize_filename("") == "untitled"

    def test_path_separators_removed(self):
        out = sanitize_filename("a/b\\c.mp3")
        assert "/" not in out and "\\" not in out

    def test_traversal_dots_stripped(self):
        out = sanitize_filename("../../etc/passwd")
        assert ".." not in out

    def test_dangerous_chars_replaced(self):
        out = sanitize_filename('a<b>c:"d|e?f*g.mp4')
        for ch in '<>:"|?*':
            assert ch not in out

    def test_null_byte_removed(self):
        assert "\0" not in sanitize_filename("bad\0name.mp3")

    def test_long_name_truncated_keeps_extension(self):
        out = sanitize_filename("x" * 500 + ".mp3", max_length=50)
        assert len(out) <= 50
        assert out.endswith(".mp3")


class TestSanitizePath:
    def test_stays_under_base(self, tmp_path):
        result = sanitize_path(tmp_path, "video.mp4")
        assert result.parent == tmp_path.resolve()

    def test_traversal_neutralized(self, tmp_path):
        # Sanitization strips separators/dots, so result remains under base
        result = sanitize_path(tmp_path, "../../../../etc/passwd")
        result.relative_to(tmp_path.resolve())  # raises if escaped


class TestSafeTempDir:
    def test_creates_directory(self, tmp_path):
        d = get_safe_temp_dir("abc123", tmp_path)
        assert d.exists() and d.is_dir()
        assert d.parent == tmp_path

    def test_strips_unsafe_task_id_chars(self, tmp_path):
        d = get_safe_temp_dir("../../evil", tmp_path)
        d.relative_to(tmp_path)  # must stay under base
        assert "/" not in d.name and ".." not in d.name
