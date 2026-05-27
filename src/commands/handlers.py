"""Command handlers for bot commands."""

from aiogram import Bot, types
from aiogram.fsm.context import FSMContext

from ..queue import get_queue_manager
from ..services.uploader import UploaderService
from ..types.download import MediaFormat
from ..utils.logger import get_logger

logger = get_logger()


# Supported platforms list
SUPPORTED_PLATFORMS = [
    "YouTube",
    "SoundCloud",
    "Vimeo",
    "TikTok",
    "Twitter/X",
    "Instagram",
    "Reddit",
    "Twitch",
    "And 1000+ more via yt-dlp",
]


class CommandHandlers:
    """Handlers for bot commands."""

    HELP_TEXT = """
<b>Media Downloader Bot</b>

Send me any media URL and I'll download and send it back to you.

<b>Commands:</b>

/start - Start the bot
/help - Show this help
/audio - Switch to audio-only mode (MP3)
/video - Switch to video download mode
/formats &lt;url&gt; - Show available formats
/cancel &lt;task_id&gt; - Cancel a download
/status - Show your active downloads

<b>Supported Platforms:</b>
"""

    def __init__(self, bot: Bot):
        self.bot = bot
        self.queue = get_queue_manager()
        self.uploader = UploaderService(bot)

    async def cmd_start(self, message: types.Message):
        """Handle /start command."""
        await message.answer(
            "👋 Welcome to Media Downloader Bot!\n\n"
            "Send me a URL to download media.\n"
            "Use /help for all commands.",
            parse_mode="HTML",
        )

    async def cmd_help(self, message: types.Message):
        """Handle /help command."""
        platforms_text = "\n".join(f"• {p}" for p in SUPPORTED_PLATFORMS)
        await message.answer(
            self.HELP_TEXT + platforms_text,
            parse_mode="HTML",
        )

    async def cmd_audio(self, message: types.Message):
        """Handle /audio command - set audio-only mode."""
        from ..bot.handlers import get_handlers, DownloadState

        user_id = message.from_user.id
        handlers = get_handlers(self.bot)
        user_state = handlers.get_user_state(user_id)
        user_state.preferred_format = MediaFormat.AUDIO

        await message.answer("🎵 Audio-only mode enabled. Downloads will be converted to MP3.")

    async def cmd_video(self, message: types.Message):
        """Handle /video command - set video mode."""
        from ..bot.handlers import get_handlers

        user_id = message.from_user.id
        handlers = get_handlers(self.bot)
        user_state = handlers.get_user_state(user_id)
        user_state.preferred_format = MediaFormat.VIDEO

        await message.answer("🎬 Video mode enabled. Downloads will include video when available.")

    async def cmd_cancel(self, message: types.Message):
        """Handle /cancel command."""
        user_id = message.from_user.id
        args = message.text.split()[1:] if len(message.text.split()) > 1 else []

        if not args:
            await message.answer(
                "Usage: /cancel &lt;task_id&gt;\n\n"
                "Use /status to see active task IDs."
            )
            return

        task_id = args[0]
        success = self.queue.cancel_task(task_id, user_id)

        if success:
            await message.answer(f"✅ Task {task_id} cancelled.")
        else:
            await message.answer(
                f"❌ Could not cancel task {task_id}.\n"
                "Make sure the task is yours and still active."
            )

    async def cmd_status(self, message: types.Message):
        """Handle /status command."""
        user_id = message.from_user.id
        summary = self.queue.get_status_summary(user_id)

        await message.answer(f"📊 Your Downloads:\n\n{summary}")

    async def cmd_formats(self, message: types.Message):
        """Handle /formats command."""
        from ..bot.handlers import get_handlers

        # Get URL from message text
        parts = message.text.split(maxsplit=1)
        if len(parts) < 2:
            await message.answer(
                "Usage: /formats &lt;url&gt;\n\n"
                "Example: /formats https://youtube.com/watch?v=..."
            )
            return

        url = parts[1].strip()
        handlers = get_handlers(self.bot)

        if not handlers.downloader.validate_url(url):
            await message.answer("❌ Invalid URL.")
            return

        status_msg = await message.answer("🔍 Fetching available formats...")

        formats = await handlers.downloader.get_formats(url)

        if not formats:
            await self.bot.edit_message_text(
                chat_id=message.chat.id,
                message_id=status_msg.message_id,
                text="❌ Could not fetch formats. The video may not be available.",
            )
            return

        output = handlers.downloader.format_formats_list(formats)
        await self.bot.edit_message_text(
            chat_id=message.chat.id,
            message_id=status_msg.message_id,
            text=f"```\n{output}\n```",
            parse_mode="Markdown",
        )
