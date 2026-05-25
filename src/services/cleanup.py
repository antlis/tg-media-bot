"""File cleanup service."""

import shutil
from pathlib import Path
from typing import List, Optional

from ..utils.logger import get_logger

logger = get_logger()


class CleanupService:
    """
    Service for cleaning up temporary files after download/upload.

    Ensures:
    - Downloaded media files are removed
    - Thumbnails are removed
    - Temporary metadata files are removed
    - Partial downloads are cleaned
    - Empty directories are removed
    """

    # File patterns to always remove
    CLEANUP_PATTERNS = {
        # yt-dlp temp files
        ".part",
        ".part-frag",
        ".temp",
        ".ytdl",
        # Thumbnails
        ".jpg",
        ".png",
        ".webp",
        # Metadata
        ".info.json",
        ".description",
        # Other temp
        ".tmp",
        ".bak",
    }

    def __init__(self, temp_base: Path):
        self.temp_base = temp_base

    def cleanup_task_dir(self, task_dir: Path) -> tuple[bool, int]:
        """
        Clean up a task's temporary directory.

        Args:
            task_dir: Directory to clean up

        Returns:
            Tuple of (success, files_removed_count)
        """
        if not task_dir.exists():
            logger.debug(f"Task dir doesn't exist: {task_dir}")
            return True, 0

        files_removed = 0

        try:
            for item in task_dir.iterdir():
                if item.is_file():
                    item.unlink()
                    files_removed += 1
                    logger.debug(f"Removed file: {item.name}")

            # Remove directory if empty
            if not any(task_dir.iterdir()):
                task_dir.rmdir()
                logger.debug(f"Removed empty dir: {task_dir}")

            return True, files_removed

        except PermissionError as e:
            logger.error(f"Permission error cleaning up {task_dir}: {e}")
            return False, files_removed
        except Exception as e:
            logger.error(f"Error cleaning up {task_dir}: {e}")
            return False, files_removed

    def cleanup_all_task_dirs(self, max_age_hours: int = 24) -> int:
        """
        Clean up all task directories older than specified hours.

        Args:
            max_age_hours: Maximum age in hours

        Returns:
            Number of directories cleaned
        """
        if not self.temp_base.exists():
            return 0

        import time
        cutoff = time.time() - (max_age_hours * 3600)
        cleaned = 0

        for item in self.temp_base.iterdir():
            if not item.is_dir():
                continue

            try:
                stat = item.stat()
                if stat.st_mtime < cutoff:
                    shutil.rmtree(item)
                    cleaned += 1
                    logger.info(f"Cleaned old task dir: {item.name}")
            except Exception as e:
                logger.error(f"Error cleaning {item}: {e}")

        return cleaned

    def get_temp_size(self) -> int:
        """Get total size of temp directory in bytes."""
        if not self.temp_base.exists():
            return 0

        total = 0
        for item in self.temp_base.rglob("*"):
            if item.is_file():
                total += item.stat().st_size
        return total

    def cleanup_partial_downloads(self) -> int:
        """
        Clean up incomplete/partial downloads.

        Returns:
            Number of partial files removed
        """
        if not self.temp_base.exists():
            return 0

        removed = 0
        for item in self.temp_base.rglob("*"):
            if item.is_file():
                name = item.name.lower()
                if any(name.endswith(p) for p in self.CLEANUP_PATTERNS):
                    try:
                        item.unlink()
                        removed += 1
                    except Exception as e:
                        logger.error(f"Error removing partial file {item}: {e}")

        return removed


# Global instance
_cleanup_service: Optional[CleanupService] = None


def get_cleanup_service() -> CleanupService:
    """Get the global cleanup service instance."""
    global _cleanup_service
    if _cleanup_service is None:
        from ..config import get_settings
        settings = get_settings()
        _cleanup_service = CleanupService(settings.temp_dir)
    return _cleanup_service
