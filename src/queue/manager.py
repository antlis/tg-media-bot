"""Async queue manager with rate limiting."""

import asyncio
import uuid
from collections import defaultdict
from dataclasses import dataclass, field
from datetime import datetime
from typing import Dict, List, Optional

from ..types.download import DownloadStatus, DownloadTask, MediaFormat
from ..utils.logger import get_logger

logger = get_logger()


@dataclass
class QueueStats:
    """Queue statistics."""

    total_queued: int = 0
    total_completed: int = 0
    total_failed: int = 0
    total_cancelled: int = 0


class QueueManager:
    """
    Async queue manager with per-user rate limiting.

    Features:
    - Async task queue
    - Per-user rate limiting
    - Task status tracking
    - Cancellation support
    """

    def __init__(
        self,
        max_parallel: int = 3,
        rate_limit_per_user: int = 2,
    ):
        self.max_parallel = max_parallel
        self.rate_limit_per_user = rate_limit_per_user

        # Task storage
        self._tasks: Dict[str, DownloadTask] = {}
        self._task_queue: asyncio.Queue[DownloadTask] = asyncio.Queue()

        # User tracking
        self._user_tasks: Dict[int, List[str]] = defaultdict(list)

        # Semaphore for concurrent downloads
        self._semaphore = asyncio.Semaphore(max_parallel)

        # Stats
        self._stats = QueueStats()

        # Worker task
        self._worker_task: Optional[asyncio.Task] = None
        self._running = False

    @property
    def stats(self) -> QueueStats:
        """Get queue statistics."""
        return self._stats

    def generate_task_id(self) -> str:
        """Generate a unique task ID."""
        return str(uuid.uuid4())[:8]

    async def add_task(
        self,
        user_id: int,
        url: str,
        preferred_format: MediaFormat = MediaFormat.AUTO,
    ) -> tuple[Optional[DownloadTask], str]:
        """
        Add a new download task to the queue.

        Returns:
            Tuple of (task, error_message). task is None if rejected.
        """
        # Check rate limit
        active_user_tasks = self.get_user_active_tasks(user_id)
        if len(active_user_tasks) >= self.rate_limit_per_user:
            self._stats.total_queued += 1
            return None, f"Rate limit reached. Max {self.rate_limit_per_user} concurrent downloads per user."

        # Check global capacity
        if self._semaphore.locked():
            # Queue if global limit reached
            pass

        # Create task
        task_id = self.generate_task_id()
        task = DownloadTask(
            task_id=task_id,
            user_id=user_id,
            url=url,
            preferred_format=preferred_format,
        )

        # Store task
        self._tasks[task_id] = task
        self._user_tasks[user_id].append(task_id)
        self._stats.total_queued += 1

        # Add to queue
        await self._task_queue.put(task)

        logger.info(
            "Task queued",
            task_id=task_id,
            user_id=user_id,
            queue_size=self._task_queue.qsize(),
        )

        return task, ""

    async def get_task(self, timeout: Optional[float] = None) -> Optional[DownloadTask]:
        """
        Get the next task from the queue.

        Acquires semaphore slot if available.
        """
        try:
            task = await asyncio.wait_for(
                self._task_queue.get(),
                timeout=timeout,
            )

            # Try to acquire semaphore slot
            if self._semaphore.locked():
                # Re-queue and wait
                await self._task_queue.put(task)
                await asyncio.sleep(0.1)
                return None

            self._semaphore.acquire()
            task.status = DownloadStatus.DOWNLOADING
            task.started_at = datetime.now()
            return task

        except asyncio.TimeoutError:
            return None

    def release_slot(self):
        """Release a semaphore slot after task completion."""
        self._semaphore.release()

    def get_task_by_id(self, task_id: str) -> Optional[DownloadTask]:
        """Get task by ID."""
        return self._tasks.get(task_id)

    def get_user_tasks(self, user_id: int) -> List[DownloadTask]:
        """Get all tasks for a user."""
        task_ids = self._user_tasks.get(user_id, [])
        return [self._tasks[tid] for tid in task_ids if tid in self._tasks]

    def get_user_active_tasks(self, user_id: int) -> List[DownloadTask]:
        """Get active (non-terminal) tasks for a user."""
        return [t for t in self.get_user_tasks(user_id) if t.is_active]

    def get_all_active_tasks(self) -> List[DownloadTask]:
        """Get all active tasks."""
        return [t for t in self._tasks.values() if t.is_active]

    def cancel_task(self, task_id: str, user_id: int) -> bool:
        """
        Cancel a task.

        Only the task owner can cancel it.
        """
        task = self._tasks.get(task_id)
        if not task:
            return False

        if task.user_id != user_id:
            return False

        if not task.is_active:
            return False

        task.status = DownloadStatus.CANCELLED
        task.completed_at = datetime.now()
        self._stats.total_cancelled += 1

        # Remove from user tasks
        if task_id in self._user_tasks.get(user_id, []):
            self._user_tasks[user_id].remove(task_id)

        logger.info("Task cancelled", task_id=task_id, user_id=user_id)
        return True

    def update_task_status(
        self,
        task_id: str,
        status: DownloadStatus,
        output_path: Optional[str] = None,
        file_size: int = 0,
        error_message: str = "",
    ):
        """Update task status and metadata."""
        task = self._tasks.get(task_id)
        if not task:
            return

        task.status = status

        if output_path:
            task.output_path = output_path
        if file_size:
            task.file_size = file_size
        if error_message:
            task.error_message = error_message

        if status in (DownloadStatus.COMPLETED, DownloadStatus.FAILED, DownloadStatus.CANCELLED):
            task.completed_at = datetime.now()
            self.release_slot()

            if status == DownloadStatus.COMPLETED:
                self._stats.total_completed += 1
            elif status == DownloadStatus.FAILED:
                self._stats.total_failed += 1

    def get_status_summary(self, user_id: Optional[int] = None) -> str:
        """Get a human-readable status summary."""
        if user_id:
            tasks = self.get_user_tasks(user_id)
            active = self.get_user_active_tasks(user_id)
        else:
            tasks = list(self._tasks.values())
            active = self.get_all_active_tasks()

        if not tasks:
            return "No downloads yet."

        lines = [
            f"Total tasks: {len(tasks)}",
            f"Active: {len(active)}/{self.rate_limit_per_user if user_id else self.max_parallel}",
        ]

        # Group by status
        by_status: Dict[DownloadStatus, int] = defaultdict(int)
        for t in tasks:
            by_status[t.status] += 1

        for status, count in sorted(by_status.items()):
            if count > 0:
                lines.append(f"  {status.value}: {count}")

        return "\n".join(lines)

    def cleanup_completed_tasks(self, older_than_hours: int = 24):
        """Remove completed tasks older than specified hours."""
        from datetime import timedelta

        cutoff = datetime.now() - timedelta(hours=older_than_hours)
        to_remove = []

        for task_id, task in self._tasks.items():
            if task.status not in (DownloadStatus.COMPLETED, DownloadStatus.FAILED, DownloadStatus.CANCELLED):
                continue
            if task.completed_at and task.completed_at < cutoff:
                to_remove.append(task_id)

        for task_id in to_remove:
            task = self._tasks.pop(task_id, None)
            if task:
                self._user_tasks[task.user_id].remove(task_id)

        if to_remove:
            logger.info(f"Cleaned up {len(to_remove)} old tasks")

    async def start(self):
        """Start the queue worker."""
        self._running = True
        logger.info("Queue manager started")

    async def stop(self):
        """Stop the queue manager."""
        self._running = False
        logger.info("Queue manager stopped")


# Global queue instance
_queue_manager: Optional[QueueManager] = None


def get_queue_manager() -> QueueManager:
    """Get the global queue manager instance."""
    global _queue_manager
    if _queue_manager is None:
        from ..config import get_settings
        settings = get_settings()
        _queue_manager = QueueManager(
            max_parallel=settings.max_parallel_downloads,
            rate_limit_per_user=settings.rate_limit_per_user,
        )
    return _queue_manager
