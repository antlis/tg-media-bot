"""Tests for logging setup, including the persistent file handler."""

import logging

from src.utils.logger import StructuredLogger


def _unique_name(tmp_path):
    # Unique logger name so the handler-dedupe guard doesn't reuse handlers
    return f"test-logger-{tmp_path.name}"


class TestFileLogging:
    def test_writes_download_lines_to_file(self, tmp_path):
        log_file = tmp_path / "logs" / "bot.log"
        log = StructuredLogger(_unique_name(tmp_path), "INFO", str(log_file))

        log.download_started(42, "https://soundcloud.com/a/b", "soundcloud", "task1")
        log.download_completed(42, "https://x", "soundcloud", "task1", 5_000_000, 12.0)

        for h in log.logger.handlers:
            h.flush()

        assert log_file.exists()
        contents = log_file.read_text()
        assert "Download started" in contents
        assert "soundcloud" in contents
        assert "task1" in contents
        assert "Download completed" in contents
        assert "file_size_mb=4.77" in contents

    def test_creates_parent_directory(self, tmp_path):
        log_file = tmp_path / "deep" / "nested" / "bot.log"
        StructuredLogger(_unique_name(tmp_path), "INFO", str(log_file))
        assert log_file.parent.exists()

    def test_no_file_handler_when_unset(self, tmp_path):
        log = StructuredLogger(_unique_name(tmp_path), "INFO", "")
        file_handlers = [
            h for h in log.logger.handlers
            if isinstance(h, logging.FileHandler)
        ]
        assert file_handlers == []
