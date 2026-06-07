"""Telegram bot handlers for download processing."""

import asyncio
import re
import uuid
from typing import Optional

from aiogram import Bot, types
from aiogram.exceptions import TelegramAPIError

from ..config import get_settings
from ..downloaders import YtDlpDownloader
from ..downloaders.ytdlp import friendly_error, render_progress_bar
from ..queue import get_queue_manager
from ..services.cleanup import get_cleanup_service
from ..services.media_cache import get_media_cache
from ..services.uploader import UploaderService, cache_entry_from_message
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
        self.cache = get_media_cache()
        self.downloader = YtDlpDownloader()
        self.uploader = UploaderService(bot)
        self._user_states: dict[int, DownloadState] = {}
        # task_id -> the background asyncio.Task running the download, so /cancel
        # can actually stop in-flight work (not just flip a status flag).
        self._running_tasks: dict[str, asyncio.Task] = {}
        # short token -> URL, for the inline quality picker (callback_data is
        # capped at 64 bytes, so the URL can't ride along in it).
        self._pending: dict[str, str] = {}

    def stash_url(self, url: str) -> str:
        """Store a URL for the quality picker and return a short token."""
        if len(self._pending) > 500:  # crude bound; tokens are short-lived
            self._pending.clear()
        token = uuid.uuid4().hex[:10]
        self._pending[token] = url
        return token

    async def on_quality_choice(self, callback) -> None:
        """Handle an inline quality-picker button press."""
        from .quality import is_valid_choice, quality_params

        data = callback.data or ""
        parts = data.split(":")  # q:<token>:<choice>
        token = parts[1] if len(parts) > 1 else ""
        choice = parts[2] if len(parts) > 2 else ""
        url = self._pending.pop(token, None)

        if not url or not is_valid_choice(choice):
            await callback.answer("This picker has expired — send the link again.", show_alert=True)
            return

        await callback.answer()
        preferred_format, max_height = quality_params(choice)
        label = f"{max_height}p" if max_height else preferred_format.value
        try:
            await callback.message.edit_text(f"✅ Selected: {label}")
        except TelegramAPIError:
            pass
        await self.enqueue_download(
            chat_id=callback.message.chat.id,
            user_id=callback.from_user.id,
            url=url,
            preferred_format=preferred_format,
            max_height=max_height,
        )

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
        """Process a download request from a plain URL message."""
        user_state = self.get_user_state(message.from_user.id)
        await self.enqueue_download(
            chat_id=message.chat.id,
            user_id=message.from_user.id,
            url=url,
            preferred_format=user_state.preferred_format,
        )

    async def enqueue_download(
        self,
        chat_id: int,
        user_id: int,
        url: str,
        preferred_format: MediaFormat,
        max_height: Optional[int] = None,
    ):
        """Validate, queue, acknowledge, and spawn a background download.

        Shared by the plain-URL path and the inline quality picker.
        """
        logger.info("Download request received", user_id=user_id, url=url[:80])

        if not self.downloader.validate_url(url):
            await self.bot.send_message(chat_id, "❌ Invalid URL. Please provide a valid media URL.")
            return

        platform = self.downloader.detect_platform(url)

        task, error = await self.queue.add_task(
            user_id=user_id,
            url=url,
            preferred_format=preferred_format,
            max_height=max_height,
        )
        if not task:
            await self.bot.send_message(chat_id, f"❌ {error}")
            return

        task.platform = platform

        quality = f"{max_height}p" if max_height else preferred_format.value
        status_msg = await self.bot.send_message(
            chat_id,
            f"⏳ Queued download...\n"
            f"Platform: {platform}\n"
            f"Task ID: `{task.task_id}`\n"
            f"Quality: {quality}",
            parse_mode="Markdown",
        )

        logger.download_started(user_id, url, platform, task.task_id)

        # Process download in background; keep a handle so /cancel can stop it.
        bg = asyncio.create_task(
            self._process_download_task(task, chat_id, status_msg.message_id)
        )
        self._running_tasks[task.task_id] = bg
        bg.add_done_callback(
            lambda _t, tid=task.task_id: self._running_tasks.pop(tid, None)
        )

    def cancel_download(self, task_id: str, user_id: int) -> bool:
        """Cancel a queued/running download.

        Marks the task cancelled (ownership-checked by the queue) and, if it's
        already running, cancels its background task so the yt-dlp subprocess is
        killed instead of finishing and uploading anyway.
        """
        if not self.queue.cancel_task(task_id, user_id):
            return False
        bg = self._running_tasks.get(task_id)
        if bg and not bg.done():
            bg.cancel()
        return True

    async def _process_download_task(self, task, chat_id: int, status_msg_id: int):
        """Background task to process a download."""
        user_id = task.user_id
        user_state = self.get_user_state(user_id)

        # Create temp directory
        temp_dir = self.settings.temp_dir / task.task_id
        temp_dir.mkdir(parents=True, exist_ok=True)

        slot_acquired = False
        try:
            # Instant re-send if we've already uploaded this URL+format.
            cached = self.cache.get(task.url, task.preferred_format.value)
            if cached:
                try:
                    msg = await self.uploader.send_cached(chat_id, cached, source_url=task.url)
                    if msg:
                        await self._update_status_message(
                            chat_id, status_msg_id,
                            f"✅ Sent (from cache).\nTask: `{task.task_id}`"
                        )
                        self.queue.update_task_status(task.task_id, DownloadStatus.COMPLETED)
                        logger.info("Served from cache", task_id=task.task_id, url=task.url[:80])
                        return
                except Exception as e:
                    # Stale file_id (rare) — drop it and download fresh.
                    logger.info(f"Cached file_id unusable, re-downloading: {e}")
                    self.cache.evict(task.url, task.preferred_format.value)

            # Wait for a global download slot (enforces MAX_PARALLEL_DOWNLOADS).
            if self.queue.slots_busy():
                await self._update_status_message(
                    chat_id, status_msg_id,
                    f"⏳ Waiting for a free download slot...\nTask: `{task.task_id}`"
                )
            await self.queue.acquire_slot()
            slot_acquired = True

            # The user may have cancelled while we waited for a slot.
            if task.status == DownloadStatus.CANCELLED:
                return

            # Update status
            task.status = DownloadStatus.DOWNLOADING
            await self._update_status_message(
                chat_id, status_msg_id,
                f"📥 Downloading...\nTask: `{task.task_id}`"
            )

            # Live progress bar (edits the same status message, throttled in
            # the downloader). Works the same in DMs and groups.
            async def on_progress(info):
                bar = render_progress_bar(info["percent"])
                meta = " · ".join(
                    p for p in (info["speed"], f"ETA {info['eta']}" if info["eta"] else "") if p
                )
                text = f"📥 Downloading...\n`{bar}` {info['percent_str']}".rstrip()
                if meta:
                    text += f"\n{meta}"
                text += f"\nTask: `{task.task_id}`"
                await self._update_status_message(chat_id, status_msg_id, text)

            # Download
            result = await self.downloader.download(
                url=task.url,
                output_dir=temp_dir,
                preferred_format=task.preferred_format,
                progress_callback=on_progress,
                max_height=task.max_height,
            )

            if not result.success:
                await self._update_status_message(
                    chat_id, status_msg_id,
                    f"❌ Download failed\n\n{friendly_error(result.error)}"
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
                performer=result.performer,
                duration=result.duration,
                thumbnail_path=result.thumbnail_path,
                source_url=task.url,
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
                # Cache the file_id so repeat requests are resent instantly.
                entry = cache_entry_from_message(upload_result)
                if entry is not None:
                    entry["title"] = entry.get("title") or result.title or ""
                    if result.performer and not entry.get("performer"):
                        entry["performer"] = result.performer
                    if result.duration and not entry.get("duration"):
                        entry["duration"] = int(result.duration)
                    self.cache.put(task.url, task.preferred_format.value, entry)
                logger.download_completed(
                    user_id, task.url, task.platform,
                    task.task_id, result.file_size,
                    task.duration or 0,
                )
            else:
                size_mb = result.file_size / (1024 * 1024)
                limit_mb = self.settings.upload_limit_mb
                error_msg = (
                    f"❌ Upload failed.\n"
                    f"File: {result.output_path.name}\n"
                    f"Size: {size_mb:.1f}MB\n\n"
                )
                if size_mb > limit_mb:
                    error_msg += f"This exceeds the {limit_mb}MB upload limit.\n"
                    if limit_mb == 50:
                        error_msg += "Run the bundled Local Bot API Server to raise it to 2GB."
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

        except asyncio.CancelledError:
            # /cancel: the download subprocess is killed downstream. Report and
            # re-raise so the cancellation isn't swallowed.
            self.queue.update_task_status(task.task_id, DownloadStatus.CANCELLED)
            try:
                await self._update_status_message(
                    chat_id, status_msg_id,
                    f"🚫 Cancelled.\nTask: `{task.task_id}`"
                )
            except TelegramAPIError:
                pass
            raise

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
            # Release the global slot if we took one, then clean up temp files.
            if slot_acquired:
                self.queue.release_slot()
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


_handlers: Optional["BotHandlers"] = None


def get_handlers(bot: Bot) -> BotHandlers:
    """Return the shared handlers instance.

    Must be a singleton: per-user state (e.g. the /audio vs /video format
    preference in ``_user_states``) has to survive across messages. Creating a
    new instance per message would reset every preference before the next
    message arrives.
    """
    global _handlers
    if _handlers is None:
        _handlers = BotHandlers(bot)
    return _handlers
