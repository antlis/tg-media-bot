"""Bot router setup."""

from aiogram import Bot, Dispatcher
from aiogram.filters import Command
from aiogram.types import Message

from ..config import get_settings
from ..utils.logger import get_logger

logger = get_logger()


async def auth_middleware(handler, event: Message, data):
    """Gate every message on the allowlist. Empty ALLOWED_USERS = open to all.

    Access is granted if the sender is an allowed user, OR the message is in a
    group that an allowed user previously activated the bot in. When an allowed
    user uses the bot in a group, that group is remembered so its other members
    can use it too.
    """
    allowed = get_settings().allowed_users
    if not allowed:
        return await handler(event, data)

    from ..services.chat_store import get_chat_store

    user = event.from_user
    chat = getattr(event, "chat", None)
    store = get_chat_store()

    user_ok = user is not None and user.id in allowed
    if user_ok:
        # Activate (and remember) the group this allowed user is using.
        if chat is not None and getattr(chat, "type", None) in ("group", "supergroup"):
            store.add(chat.id)
        return await handler(event, data)

    if chat is not None and store.contains(chat.id):
        return await handler(event, data)

    logger.warning(f"Blocked unauthorized user: {user.id if user else 'unknown'}")
    await event.answer("⛔ You are not authorized to use this bot.")
    return


def create_router(bot: Bot) -> Dispatcher:
    """Create and configure the dispatcher with all handlers."""
    dp = Dispatcher()

    # Gate every message on the allowlist before any handler runs
    dp.message.outer_middleware(auth_middleware)

    # Register commands
    dp.message.register(cmd_start, Command("start"))
    dp.message.register(cmd_help, Command("help"))
    dp.message.register(cmd_audio, Command("audio"))
    dp.message.register(cmd_video, Command("video"))
    dp.message.register(cmd_cancel, Command("cancel"))
    dp.message.register(cmd_status, Command("status"))
    dp.message.register(cmd_formats, Command("formats"))

    # Handle text messages (URLs)
    @dp.message()
    async def handle_text(message):
        from .handlers import get_handlers
        logger.info(f"Message received: {message.text[:50] if message.text else 'empty'}")
        handlers = get_handlers(bot)
        await handlers.handle_message(message)

    return dp


async def cmd_start(message, bot: Bot):
    """Handle /start command."""
    from ..commands import CommandHandlers
    logger.info(f"/start received from user {message.from_user.id}")
    ch = CommandHandlers(bot)
    await ch.cmd_start(message)


async def cmd_help(message, bot: Bot):
    """Handle /help command."""
    from ..commands import CommandHandlers
    logger.info(f"/help received from user {message.from_user.id}")
    ch = CommandHandlers(bot)
    await ch.cmd_help(message)


async def cmd_audio(message, bot: Bot):
    """Handle /audio command."""
    from ..commands import CommandHandlers
    ch = CommandHandlers(bot)
    await ch.cmd_audio(message)


async def cmd_video(message, bot: Bot):
    """Handle /video command."""
    from ..commands import CommandHandlers
    ch = CommandHandlers(bot)
    await ch.cmd_video(message)


async def cmd_cancel(message, bot: Bot):
    """Handle /cancel command."""
    from ..commands import CommandHandlers
    ch = CommandHandlers(bot)
    await ch.cmd_cancel(message)


async def cmd_status(message, bot: Bot):
    """Handle /status command."""
    from ..commands import CommandHandlers
    ch = CommandHandlers(bot)
    await ch.cmd_status(message)


async def cmd_formats(message, bot: Bot):
    """Handle /formats command."""
    from ..commands import CommandHandlers
    ch = CommandHandlers(bot)
    await ch.cmd_formats(message)