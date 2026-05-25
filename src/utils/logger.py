"""Structured logging utilities."""

import logging
import sys
from datetime import datetime
from enum import Enum
from pathlib import Path
from typing import Optional


class LogLevel(str, Enum):
    """Log level names."""

    DEBUG = "DEBUG"
    INFO = "INFO"
    WARNING = "WARNING"
    ERROR = "ERROR"


class StructuredLogger:
    """Logger with structured output for better log parsing."""

    def __init__(self, name: str, level: str = "INFO"):
        self.logger = logging.getLogger(name)
        self.logger.setLevel(getattr(logging, level.upper(), logging.INFO))

        if not self.logger.handlers:
            handler = logging.StreamHandler(sys.stdout)
            handler.setLevel(logging.DEBUG)
            formatter = logging.Formatter(
                "%(asctime)s | %(levelname)-8s | %(name)s | %(message)s",
                datefmt="%Y-%m-%d %H:%M:%S",
            )
            handler.setFormatter(formatter)
            self.logger.addHandler(handler)

    def _log(
        self,
        level: int,
        msg: str,
        extra: Optional[dict] = None,
    ):
        """Log with optional structured extra data."""
        if extra:
            extra_str = " | ".join(f"{k}={v}" for k, v in extra.items() if v is not None)
            msg = f"{msg} | {extra_str}"
        self.logger.log(level, msg)

    def debug(self, msg: str, **kwargs):
        """Log debug message."""
        self._log(logging.DEBUG, msg, kwargs)

    def info(self, msg: str, **kwargs):
        """Log info message."""
        self._log(logging.INFO, msg, kwargs)

    def warning(self, msg: str, **kwargs):
        """Log warning message."""
        self._log(logging.WARNING, msg, kwargs)

    def error(self, msg: str, **kwargs):
        """Log error message."""
        self._log(logging.ERROR, msg, kwargs)

    def download_started(self, user_id: int, url: str, platform: str, task_id: str):
        """Log download start."""
        self.info(
            "Download started",
            user_id=user_id,
            url=url[:100],  # Truncate long URLs
            platform=platform,
            task_id=task_id,
        )

    def download_completed(
        self,
        user_id: int,
        url: str,
        platform: str,
        task_id: str,
        file_size: int,
        duration: float,
    ):
        """Log successful download."""
        self.info(
            "Download completed",
            user_id=user_id,
            platform=platform,
            task_id=task_id,
            file_size_mb=round(file_size / (1024 * 1024), 2),
            duration_sec=round(duration, 2),
        )

    def download_failed(
        self,
        user_id: int,
        url: str,
        platform: str,
        task_id: str,
        error: str,
    ):
        """Log failed download."""
        self.error(
            "Download failed",
            user_id=user_id,
            platform=platform,
            task_id=task_id,
            error=error[:200],
        )

    def cleanup_completed(self, task_id: str, success: bool, files_removed: int):
        """Log cleanup result."""
        self.info(
            "Cleanup completed",
            task_id=task_id,
            cleanup_success=success,
            files_removed=files_removed,
        )

    def rate_limit_hit(self, user_id: int, active_tasks: int, limit: int):
        """Log rate limit hit."""
        self.warning(
            "Rate limit reached",
            user_id=user_id,
            active_tasks=active_tasks,
            limit=limit,
        )


# Global logger instance
_logger: Optional[StructuredLogger] = None


def setup_logging(level: str = "INFO") -> StructuredLogger:
    """Setup and return the global logger."""
    global _logger
    _logger = StructuredLogger("tg-media-bot", level)
    return _logger


def get_logger() -> StructuredLogger:
    """Get the global logger instance."""
    global _logger
    if _logger is None:
        _logger = setup_logging()
    return _logger
