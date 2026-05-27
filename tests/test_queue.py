"""Tests for the async queue manager (rate limiting, cancel, status)."""

import pytest

from src.queue.manager import QueueManager
from src.types.download import DownloadStatus, MediaFormat


@pytest.fixture
def queue():
    return QueueManager(max_parallel=3, rate_limit_per_user=2)


class TestAddTask:
    async def test_add_returns_task(self, queue):
        task, err = await queue.add_task(user_id=1, url="https://x")
        assert task is not None and err == ""
        assert task.user_id == 1
        assert queue.get_task_by_id(task.task_id) is task

    async def test_rate_limit_enforced(self, queue):
        await queue.add_task(1, "https://a")
        await queue.add_task(1, "https://b")
        task, err = await queue.add_task(1, "https://c")  # 3rd, limit is 2
        assert task is None
        assert "Rate limit" in err

    async def test_rate_limit_is_per_user(self, queue):
        await queue.add_task(1, "https://a")
        await queue.add_task(1, "https://b")
        task, err = await queue.add_task(2, "https://c")  # different user
        assert task is not None


class TestCancel:
    async def test_owner_can_cancel(self, queue):
        task, _ = await queue.add_task(1, "https://a")
        assert queue.cancel_task(task.task_id, user_id=1) is True
        assert queue.get_task_by_id(task.task_id).status == DownloadStatus.CANCELLED

    async def test_non_owner_cannot_cancel(self, queue):
        task, _ = await queue.add_task(1, "https://a")
        assert queue.cancel_task(task.task_id, user_id=2) is False

    async def test_unknown_task(self, queue):
        assert queue.cancel_task("deadbeef", user_id=1) is False

    async def test_cancel_frees_user_slot(self, queue):
        t1, _ = await queue.add_task(1, "https://a")
        await queue.add_task(1, "https://b")
        queue.cancel_task(t1.task_id, 1)
        # One active slot freed → can add again
        task, err = await queue.add_task(1, "https://c")
        assert task is not None


class TestStatus:
    async def test_empty_summary(self, queue):
        assert queue.get_status_summary(user_id=1) == "No downloads yet."

    async def test_summary_counts(self, queue):
        await queue.add_task(1, "https://a")
        summary = queue.get_status_summary(user_id=1)
        assert "Total tasks: 1" in summary

    async def test_update_status_completed_updates_stats(self, queue):
        task, _ = await queue.add_task(1, "https://a")
        queue.update_task_status(task.task_id, DownloadStatus.COMPLETED,
                                 output_path="/tmp/x.mp4", file_size=123)
        assert queue.stats.total_completed == 1
        assert queue.get_task_by_id(task.task_id).file_size == 123
