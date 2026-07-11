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
/formats &lt;url&gt; - Pick a download quality (buttons)
/cancel &lt;task_id&gt; - Cancel a download
/status - Show your active downloads
/minimal on|off - Toggle minimal UI (no status messages, no caption on media)

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
        from ..bot.handlers import get_handlers
        success = get_handlers(self.bot).cancel_download(task_id, user_id)

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

    async def cmd_minimal(self, message: types.Message):
        """Handle /minimal command - toggle minimal UI mode for this chat.

        In minimal mode, downloads produce no queued/progress/"Done!" status
        messages and the uploaded media carries no title/source-URL caption —
        just the file itself. Failures are still reported.
        """
        from ..services.minimal_store import get_minimal_store

        store = get_minimal_store()
        chat_id = message.chat.id
        args = message.text.split()[1:] if len(message.text.split()) > 1 else []

        if not args:
            enabled = store.contains(chat_id)
            await message.answer(
                f"Minimal UI mode is currently {'ON' if enabled else 'OFF'} for this chat.\n"
                "Usage: /minimal on|off"
            )
            return

        choice = args[0].lower()
        if choice not in ("on", "off"):
            await message.answer("Usage: /minimal on|off")
            return

        store.set(chat_id, choice == "on")
        if choice == "on":
            await message.answer(
                "🤫 Minimal UI mode enabled. Downloads will be sent with no "
                "status messages and no caption."
            )
        else:
            await message.answer("✅ Minimal UI mode disabled. Normal status messages and captions restored.")

    async def cmd_formats(self, message: types.Message):
        """Handle /formats — show an inline quality picker for a URL."""
        from aiogram.types import InlineKeyboardButton, InlineKeyboardMarkup
        from ..bot.handlers import get_handlers
        from ..bot.quality import QUALITY_CHOICES

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

        token = handlers.stash_url(url)
        keyboard = InlineKeyboardMarkup(inline_keyboard=[
            [InlineKeyboardButton(text=label, callback_data=f"q:{token}:{value}")]
            for label, value in QUALITY_CHOICES
        ])
        await message.answer("🎚️ Choose a quality:", reply_markup=keyboard)
