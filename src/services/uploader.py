"""Telegram upload service."""

import asyncio
import html
from pathlib import Path
from typing import Optional, Union

from aiogram import Bot
from aiogram.types import FSInputFile as InputFile, InputMedia, Message

from ..types.download import MediaFormat
from ..utils.logger import get_logger

logger = get_logger()

# Telegram thumbnail constraints: JPEG, <200KB, max 320x320
_THUMB_MAX_BYTES = 200 * 1024

# Telegram caption hard limit
_CAPTION_MAX = 1024


def _build_caption(title: str, source_url: Optional[str]) -> Optional[str]:
    """Build an HTML caption: title plus the source URL as monospace text.

    The URL is wrapped in <code> so Telegram renders it as plain (copyable)
    text — not an auto-linked hyperlink — and does not fetch a link preview.
    Returns HTML; callers must send with parse_mode="HTML".
    """
    parts: list[str] = []
    if source_url:
        # Reserve room for the URL line so a long title can't crowd it out
        budget = _CAPTION_MAX - len(source_url) - 32
        if title and budget > 0:
            parts.append(html.escape(title.strip())[:budget])
        parts.append(f"<code>{html.escape(source_url)}</code>")
    elif title:
        parts.append(html.escape(title.strip())[:_CAPTION_MAX])

    return "\n\n".join(parts) if parts else None


async def _prepare_thumbnail(src: Path) -> Optional[Path]:
    """Resize cover art to Telegram's thumbnail limits via ffmpeg.

    Returns a path to a <200KB JPEG no larger than 320x320, or None if the
    source is missing or ffmpeg fails. Failure is non-fatal — the audio still
    uploads without a thumbnail.
    """
    if not src or not src.exists():
        return None

    dst = src.with_name(f"{src.stem}_tg.jpg")
    cmd = [
        "ffmpeg", "-y", "-i", str(src),
        "-vf", "scale=320:320:force_original_aspect_ratio=decrease",
        "-q:v", "5",
        str(dst),
    ]
    try:
        proc = await asyncio.create_subprocess_exec(
            *cmd,
            stdout=asyncio.subprocess.DEVNULL,
            stderr=asyncio.subprocess.PIPE,
        )
        _, stderr = await proc.communicate()
        if proc.returncode != 0 or not dst.exists():
            logger.debug(f"Thumbnail resize failed: {stderr.decode(errors='replace')[:200]}")
            return None
        if dst.stat().st_size > _THUMB_MAX_BYTES:
            logger.debug("Resized thumbnail still over 200KB; skipping thumbnail")
            return None
        return dst
    except Exception as e:
        logger.debug(f"Thumbnail prep error: {e}")
        return None


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
        source_url: Optional[str] = None,
        reply_to_message_id: Optional[int] = None,
        supports_streaming: bool = True,
    ) -> Optional[Message]:
        """
        Upload a video to Telegram.

        Args:
            chat_id: Target chat ID
            file_path: Path to video file
            caption: Optional caption (title text)
            source_url: Original media URL, shown as plain text in the caption
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
                caption=_build_caption(caption, source_url),
                parse_mode="HTML",
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
            logger.error(f"Video upload failed, trying as document: {e}")
            # Fall back to document upload for large files
            return await self.upload_document(
                chat_id=chat_id,
                file_path=file_path,
                caption=caption,
                source_url=source_url,
                reply_to_message_id=reply_to_message_id,
            )

    async def upload_audio(
        self,
        chat_id: int,
        file_path: Path,
        caption: str = "",
        title: Optional[str] = None,
        performer: Optional[str] = None,
        duration: float = 0.0,
        thumbnail_path: Optional[Path] = None,
        source_url: Optional[str] = None,
        reply_to_message_id: Optional[int] = None,
    ) -> Optional[Message]:
        """
        Upload an audio file to Telegram.

        Args:
            chat_id: Target chat ID
            file_path: Path to audio file
            caption: Optional caption (title text)
            title: Audio title (for music)
            performer: Audio performer (for music)
            duration: Track length in seconds (shown by Telegram)
            thumbnail_path: Optional cover-art image to attach
            source_url: Original media URL, shown as plain text in the caption
            reply_to_message_id: Optional message to reply to

        Returns:
            Sent message or None on failure
        """
        if not file_path.exists():
            logger.error(f"File not found: {file_path}")
            return None

        thumb = await _prepare_thumbnail(thumbnail_path) if thumbnail_path else None

        try:
            input_file = InputFile(file_path)

            message = await self.bot.send_audio(
                chat_id=chat_id,
                audio=input_file,
                caption=_build_caption(caption, source_url),
                parse_mode="HTML",
                title=title,
                performer=performer,
                duration=int(duration) if duration else None,
                thumbnail=InputFile(thumb) if thumb else None,
                reply_to_message_id=reply_to_message_id,
            )

            logger.info(
                f"Audio uploaded: {file_path.name}",
                size_mb=file_path.stat().st_size / (1024 * 1024),
                message_id=message.message_id,
                thumbnail=bool(thumb),
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
        source_url: Optional[str] = None,
        reply_to_message_id: Optional[int] = None,
    ) -> Optional[Message]:
        """
        Upload a file as document.

        Args:
            chat_id: Target chat ID
            file_path: Path to file
            caption: Optional caption (title text)
            source_url: Original media URL, shown as plain text in the caption
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
                caption=_build_caption(caption, source_url),
                parse_mode="HTML",
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
        performer: str = "",
        duration: float = 0.0,
        thumbnail_path: Optional[Path] = None,
        source_url: Optional[str] = None,
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
            performer: Audio performer (audio only)
            duration: Track length in seconds (audio only)
            thumbnail_path: Optional cover art (audio only)
            source_url: Original media URL, shown as plain text in the caption
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
                performer=performer or None,
                duration=duration,
                thumbnail_path=thumbnail_path,
                source_url=source_url,
                reply_to_message_id=reply_to_message_id,
            )

        # Video formats
        video_formats = {".mp4", ".mkv", ".webm", ".avi", ".mov"}

        if suffix in video_formats or media_format == MediaFormat.VIDEO:
            return await self.upload_video(
                chat_id=chat_id,
                file_path=file_path,
                caption=title,
                source_url=source_url,
                reply_to_message_id=reply_to_message_id,
            )

        # Default to document for unknown formats
        return await self.upload_document(
            chat_id=chat_id,
            file_path=file_path,
            caption=title,
            source_url=source_url,
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
