"""Telegram upload service."""

import asyncio
from pathlib import Path
from typing import Optional, Union

from aiogram import Bot
from aiogram.types import FSInputFile as InputFile, InputMedia, Message

from ..types.download import MediaFormat
from ..utils.logger import get_logger

logger = get_logger()


class UploaderService:
    """
    Service for uploading files to Telegram.

    Handles:
    - Video uploads
    - Audio uploads
    - Captioned media
    - Progress tracking
    - Error handling
    """

    def __init__(self, bot: Bot):
        self.bot = bot

    async def upload_video(
        self,
        chat_id: int,
        file_path: Path,
        caption: str = "",
        reply_to_message_id: Optional[int] = None,
        supports_streaming: bool = True,
    ) -> Optional[Message]:
        """
        Upload a video to Telegram.

        Args:
            chat_id: Target chat ID
            file_path: Path to video file
            caption: Optional caption
            reply_to_message_id: Optional message to reply to
            supports_streaming: Enable streaming for large videos

        Returns:
            Sent message or None on failure
        """
        if not file_path.exists():
            logger.error(f"File not found: {file_path}")
            return None

        try:
            input_file = InputFile(file_path)

            message = await self.bot.send_video(
                chat_id=chat_id,
                video=input_file,
                caption=caption[:1024] if caption else None,
                reply_to_message_id=reply_to_message_id,
                supports_streaming=supports_streaming,
            )

            logger.info(
                f"Video uploaded: {file_path.name}",
                size_mb=file_path.stat().st_size / (1024 * 1024),
                message_id=message.message_id,
            )

            return message

        except Exception as e:
            logger.error(f"Video upload failed: {e}")
            return None

    async def upload_audio(
        self,
        chat_id: int,
        file_path: Path,
        caption: str = "",
        title: Optional[str] = None,
        performer: Optional[str] = None,
        reply_to_message_id: Optional[int] = None,
    ) -> Optional[Message]:
        """
        Upload an audio file to Telegram.

        Args:
            chat_id: Target chat ID
            file_path: Path to audio file
            caption: Optional caption
            title: Audio title (for music)
            performer: Audio performer (for music)
            reply_to_message_id: Optional message to reply to

        Returns:
            Sent message or None on failure
        """
        if not file_path.exists():
            logger.error(f"File not found: {file_path}")
            return None

        try:
            input_file = InputFile(file_path)

            message = await self.bot.send_audio(
                chat_id=chat_id,
                audio=input_file,
                caption=caption[:1024] if caption else None,
                title=title,
                performer=performer,
                reply_to_message_id=reply_to_message_id,
            )

            logger.info(
                f"Audio uploaded: {file_path.name}",
                size_mb=file_path.stat().st_size / (1024 * 1024),
                message_id=message.message_id,
            )

            return message

        except Exception as e:
            logger.error(f"Audio upload failed: {e}")
            return None

    async def upload_document(
        self,
        chat_id: int,
        file_path: Path,
        caption: str = "",
        reply_to_message_id: Optional[int] = None,
    ) -> Optional[Message]:
        """
        Upload a file as document.

        Args:
            chat_id: Target chat ID
            file_path: Path to file
            caption: Optional caption
            reply_to_message_id: Optional message to reply to

        Returns:
            Sent message or None on failure
        """
        if not file_path.exists():
            logger.error(f"File not found: {file_path}")
            return None

        try:
            input_file = InputFile(file_path)

            message = await self.bot.send_document(
                chat_id=chat_id,
                document=input_file,
                caption=caption[:1024] if caption else None,
                reply_to_message_id=reply_to_message_id,
            )

            logger.info(
                f"Document uploaded: {file_path.name}",
                size_mb=file_path.stat().st_size / (1024 * 1024),
                message_id=message.message_id,
            )

            return message

        except Exception as e:
            logger.error(f"Document upload failed: {e}")
            return None

    async def upload_media(
        self,
        chat_id: int,
        file_path: Path,
        media_format: MediaFormat,
        title: str = "",
        reply_to_message_id: Optional[int] = None,
    ) -> Optional[Message]:
        """
        Upload media based on format preference.

        Automatically determines best upload method based on file extension.

        Args:
            chat_id: Target chat ID
            file_path: Path to media file
            media_format: Preferred format (video/audio/auto)
            title: Optional title for media
            reply_to_message_id: Optional message to reply to

        Returns:
            Sent message or None on failure
        """
        suffix = file_path.suffix.lower()

        # Audio formats
        audio_formats = {".mp3", ".m4a", ".wav", ".flac", ".ogg", ".aac"}

        if suffix in audio_formats or media_format == MediaFormat.AUDIO:
            return await self.upload_audio(
                chat_id=chat_id,
                file_path=file_path,
                caption=title,
                title=title,
                reply_to_message_id=reply_to_message_id,
            )

        # Video formats
        video_formats = {".mp4", ".mkv", ".webm", ".avi", ".mov"}

        if suffix in video_formats or media_format == MediaFormat.VIDEO:
            return await self.upload_video(
                chat_id=chat_id,
                file_path=file_path,
                caption=title,
                reply_to_message_id=reply_to_message_id,
            )

        # Default to document for unknown formats
        return await self.upload_document(
            chat_id=chat_id,
            file_path=file_path,
            caption=title,
            reply_to_message_id=reply_to_message_id,
        )

    async def send_progress_message(
        self,
        chat_id: int,
        text: str,
        reply_to_message_id: Optional[int] = None,
    ) -> Optional[Message]:
        """Send a simple progress/status message."""
        try:
            return await self.bot.send_message(
                chat_id=chat_id,
                text=text,
                reply_to_message_id=reply_to_message_id,
            )
        except Exception as e:
            logger.error(f"Failed to send message: {e}")
            return None

    async def edit_message(
        self,
        chat_id: int,
        message_id: int,
        text: str,
    ) -> bool:
        """Edit an existing message."""
        try:
            await self.bot.edit_message_text(
                chat_id=chat_id,
                message_id=message_id,
                text=text,
            )
            return True
        except Exception as e:
            logger.error(f"Failed to edit message: {e}")
            return False
