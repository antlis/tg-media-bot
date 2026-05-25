"""Telegram bot handlers for download processing."""

import asyncio
import re
import uuid
from datetime import datetime
from pathlib import Path
from typing import Optional

from aiogram import Bot, Dispatcher, types
from aiogram.fsm.context import FSMContext
from aiogram.exceptions import TelegramAPIError

from ..config import get_settings
from ..downloaders import YtDlpDownloader
from ..queue import get_queue_manager
from ..services.cleanup import get_cleanup_service
from ..services.uploader import UploaderService
from ..types.download import DownloadStatus, MediaFormat
from ..utils.logger import get_logger

logger = get_logger()


class DownloadState:
    """User state for download operations."""

    # Per-user format preference
    preferred_format: MediaFormat = MediaFormat.AUTO

    # Track current task being processed
    current_task_id: Optional[str] = None


class BotHandlers:
    """Handlers for bot commands and messages."""

    def __init__(self, bot: Bot):
        self.bot = bot
        self.settings = get_settings()
        self.queue = get_queue_manager()
        self.cleanup = get_cleanup_service()
        self.downloader = YtDlpDownloader()
        self.uploader = UploaderService(bot)
        self._user_states: dict[int, DownloadState] = {}

    def get_user_state(self, user_id: int) -> DownloadState:
        """Get or create user state."""
        if user_id not in self._user_states:
            self._user_states[user_id] = DownloadState()
        return self._user_states[user_id]

    def extract_urls(self, text: str) -> list[str]:
        """Extract URLs from text."""
        if not text:
            return []

        url_pattern = r"https?://[^\s<>\[\]{}\"|]+"
        urls = re.findall(url_pattern, text)
        return [u.rstrip(".,;:!?") for u in urls if len(u) < 2048]

    async def process_download(self, message: types.Message, url: str):
        """Process a download request."""
        user_id = message.from_user.id
        user_state = self.get_user_state(user_id)

        logger.info(
            "Download request received",
            user_id=user_id,
            url=url[:80],
        )

        # Validate URL
        if not self.downloader.validate_url(url):
            await message.answer("❌ Invalid URL. Please provide a valid media URL.")
            return

        # Detect platform
        platform = self.downloader.detect_platform(url)

        # Add to queue
        task, error = await self.queue.add_task(
            user_id=user_id,
            url=url,
            preferred_format=user_state.preferred_format,
        )

        if not task:
            await message.answer(f"❌ {error}")
            return

        task.platform = platform

        # Acknowledge
        status_msg = await message.answer(
            f"⏳ Queued download...\n"
            f"Platform: {platform}\n"
            f"Task ID: `{task.task_id}`\n"
            f"Format: {user_state.preferred_format.value}",
            parse_mode="Markdown",
        )

        logger.download_started(user_id, url, platform, task.task_id)

        # Process download in background
        asyncio.create_task(
            self._process_download_task(task, message.chat.id, status_msg.message_id)
        )

    async def _process_download_task(self, task, chat_id: int, status_msg_id: int):
        """Background task to process a download."""
        user_id = task.user_id
        user_state = self.get_user_state(user_id)

        # Create temp directory
        temp_dir = self.settings.temp_dir / task.task_id
        temp_dir.mkdir(parents=True, exist_ok=True)

        try:
            # Update status
            task.status = DownloadStatus.DOWNLOADING
            await self._update_status_message(
                chat_id, status_msg_id,
                f"📥 Downloading...\nTask: `{task.task_id}`"
            )

            # Download
            result = await self.downloader.download(
                url=task.url,
                output_dir=temp_dir,
                preferred_format=task.preferred_format,
            )

            if not result.success:
                await self._update_status_message(
                    chat_id, status_msg_id,
                    f"❌ Download failed\n\n{result.error[:500]}"
                )
                self.queue.update_task_status(
                    task.task_id,
                    DownloadStatus.FAILED,
                    error_message=result.error,
                )
                logger.download_failed(user_id, task.url, task.platform, task.task_id, result.error)
                return

            # Update with result
            task.output_path = result.output_path
            task.file_size = result.file_size
            task.status = DownloadStatus.PROCESSING

            await self._update_status_message(
                chat_id, status_msg_id,
                f"📤 Uploading...\n{result.output_path.name}\n"
                f"Size: {result.file_size / (1024*1024):.1f}MB"
            )

            # Upload
            upload_result = await self.uploader.upload_media(
                chat_id=chat_id,
                file_path=result.output_path,
                media_format=task.preferred_format,
                title=result.title,
            )

            if upload_result:
                await self._update_status_message(
                    chat_id, status_msg_id,
                    f"✅ Done!\n"
                    f"{result.output_path.name}\n"
                    f"Size: {result.file_size / (1024*1024):.1f}MB"
                )
                self.queue.update_task_status(
                    task.task_id,
                    DownloadStatus.COMPLETED,
                    str(result.output_path),
                    result.file_size,
                )
                logger.download_completed(
                    user_id, task.url, task.platform,
                    task.task_id, result.file_size,
                    task.duration or 0,
                )
            else:
                size_mb = result.file_size / (1024 * 1024)
                error_msg = (
                    f"❌ Upload failed.\n"
                    f"File: {result.output_path.name}\n"
                    f"Size: {size_mb:.1f}MB\n\n"
                )
                if size_mb > 50:
                    error_msg += (
                        "Telegram Bot API has a 50MB upload limit.\n"
                        "Consider using a Local Bot API Server for larger files."
                    )
                else:
                    error_msg += "Telegram rejected the file."
                await self._update_status_message(
                    chat_id, status_msg_id,
                    error_msg,
                )
                self.queue.update_task_status(
                    task.task_id,
                    DownloadStatus.FAILED,
                    error_message=f"Upload failed ({size_mb:.1f}MB)",
                )

        except Exception as e:
            logger.error(f"Download task error: {e}", task_id=task.task_id)
            self.queue.update_task_status(
                task.task_id,
                DownloadStatus.FAILED,
                error_message=str(e),
            )
            try:
                await self._update_status_message(
                    chat_id, status_msg_id,
                    f"❌ Error: {str(e)[:500]}"
                )
            except TelegramAPIError:
                pass

        finally:
            # Cleanup
            success, count = self.cleanup.cleanup_task_dir(temp_dir)
            logger.cleanup_completed(task.task_id, success, count)

    async def _update_status_message(self, chat_id: int, message_id: int, text: str):
        """Edit status message."""
        try:
            await self.bot.edit_message_text(
                chat_id=chat_id,
                message_id=message_id,
                text=text,
                parse_mode="Markdown",
            )
        except TelegramAPIError:
            pass

    async def handle_message(self, message: types.Message):
        """Handle regular messages with URLs."""
        urls = self.extract_urls(message.text)
        if not urls:
            await message.answer(
                "👋 Send me a URL to download media.\n"
                "Use /help for available commands."
            )
            return

        for url in urls[:3]:  # Limit to 3 URLs per message
            await self.process_download(message, url)

    async def handle_format_command(self, message: types.Message):
        """Handle /formats command to show available formats."""
        urls = self.extract_urls(message.text)
        if not urls:
            await message.answer("Please provide a URL with /formats.\nExample: /formats https://youtube.com/watch?v=...")
            return

        url = urls[0]
        status_msg = await message.answer("🔍 Fetching available formats...")

        formats = await self.downloader.get_formats(url)

        if not formats:
            await self.bot.edit_message_text(
                chat_id=message.chat.id,
                message_id=status_msg.message_id,
                text="❌ Could not fetch formats. The video may not be available.",
            )
            return

        output = self.downloader.format_formats_list(formats)
        await self.bot.edit_message_text(
            chat_id=message.chat.id,
            message_id=status_msg.message_id,
            text=f"```\n{output}\n```",
            parse_mode="Markdown",
        )


def get_handlers(bot: Bot) -> BotHandlers:
    """Factory to create handlers instance."""
    return BotHandlers(bot)
