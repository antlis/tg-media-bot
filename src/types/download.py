"""Download-related types."""

import enum
from dataclasses import dataclass, field
from datetime import datetime
from pathlib import Path
from typing import Optional


class DownloadStatus(str, enum.Enum):
    """Status of a download task."""

    QUEUED = "queued"
    DOWNLOADING = "downloading"
    PROCESSING = "processing"
    UPLOADING = "uploading"
    COMPLETED = "completed"
    FAILED = "failed"
    CANCELLED = "cancelled"


class MediaFormat(str, enum.Enum):
    """Preferred media format."""

    VIDEO = "video"
    AUDIO = "audio"
    AUTO = "auto"


@dataclass
class DownloadTask:
    """Represents a download task."""

    task_id: str
    user_id: int
    url: str
    platform: str = ""
    status: DownloadStatus = DownloadStatus.QUEUED
    preferred_format: MediaFormat = MediaFormat.AUTO
    output_path: Optional[Path] = None
    file_size: int = 0
    error_message: str = ""
    started_at: Optional[datetime] = None
    completed_at: Optional[datetime] = None
    created_at: datetime = field(default_factory=datetime.now)

    def __post_init__(self):
        """Ensure enums are proper enum instances."""
        if isinstance(self.status, str):
            self.status = DownloadStatus(self.status)
        if isinstance(self.preferred_format, str):
            self.preferred_format = MediaFormat(self.preferred_format)

    @property
    def duration(self) -> Optional[float]:
        """Get download duration in seconds."""
        if self.started_at and self.completed_at:
            return (self.completed_at - self.started_at).total_seconds()
        return None

    @property
    def is_active(self) -> bool:
        """Check if task is currently active."""
        return self.status in (
            DownloadStatus.QUEUED,
            DownloadStatus.DOWNLOADING,
            DownloadStatus.PROCESSING,
            DownloadStatus.UPLOADING,
        )
