#!/usr/bin/env python3
"""
tg-media-bot - Telegram Media Downloader Bot

A lightweight self-hosted Telegram bot for downloading media from various platforms.
"""

import asyncio
import signal
import sys
from pathlib import Path

import structlog

# Add src to path
sys.path.insert(0, str(Path(__file__).parent))

from aiogram import Bot, Dispatcher
from aiogram.client.session.aiohttp import AiohttpSession
from aiogram.client.telegram import TelegramAPIServer
from aiogram.exceptions import TelegramAPIError

from src.config import get_settings
from src.utils.logger import setup_logging, get_logger
from src.queue import get_queue_manager
from src.services.cleanup import get_cleanup_service
from src.bot.router import create_router


async def shutdown(sig, loop, bot: Bot, queue, cleanup):
    """Graceful shutdown handler."""
    logger = get_logger()
    logger.info(f"Received signal {sig.name}, shutting down...")

    # Cancel all tasks
    tasks = [t for t in asyncio.all_tasks() if t is not asyncio.current_task()]
    for task in tasks:
        task.cancel()

    # Stop queue
    await queue.stop()

    # Final cleanup
    logger.info("Performing final cleanup...")
    cleaned = cleanup.cleanup_all_task_dirs(max_age_hours=0)
    logger.info(f"Final cleanup: {cleaned} directories removed")

    # Close bot session
    try:
        await bot.session.close()
    except Exception as e:
        logger.error(f"Error closing bot session: {e}")

    logger.info("Shutdown complete")


async def main():
    """Main entry point."""
    # Load settings
    try:
        settings = get_settings()
    except ValueError as e:
        print(f"Configuration error: {e}")
        print("Please set BOT_TOKEN in your .env file")
        sys.exit(1)

    # Setup logging
    logger = setup_logging(settings.log_level, settings.log_file)
    logger.info("Starting tg-media-bot")
    if settings.log_file:
        logger.info(f"Persisting logs to: {settings.log_file}")
    logger.info(f"Temp directory: {settings.temp_dir}")
    logger.info(f"Max parallel downloads: {settings.max_parallel_downloads}")

    # Initialize queue
    queue = get_queue_manager()
    await queue.start()

    # Initialize cleanup service
    cleanup = get_cleanup_service()

    # Create bot (use local API server if configured)
    if settings.api_server_url:
        logger.info(f"Using Local Bot API Server: {settings.api_server_url}")
        api_server = TelegramAPIServer.from_base(settings.api_server_url)
        session = AiohttpSession(api=api_server)
        bot = Bot(token=settings.bot_token, session=session)
    else:
        logger.info("Using standard Telegram Bot API (50MB upload limit)")
        bot = Bot(token=settings.bot_token)

    # Create dispatcher
    dp = create_router(bot)

    # Setup shutdown handlers
    loop = asyncio.get_event_loop()
    for sig in (signal.SIGTERM, signal.SIGINT):
        loop.add_signal_handler(
            sig,
            lambda s=sig: asyncio.create_task(shutdown(s, loop, bot, queue, cleanup))
        )

    logger.info("Bot initialized, starting polling...")

    try:
        # Start polling with bot
        await dp.start_polling(
            bot,
            timeout=60,
            relax=0.1,
            fast_update=True,
        )
    except TelegramAPIError as e:
        logger.error(f"Telegram API error: {e}")
        sys.exit(1)
    except Exception as e:
        logger.error(f"Unexpected error: {e}")
        sys.exit(1)
    finally:
        await bot.session.close()


if __name__ == "__main__":
    asyncio.run(main())
