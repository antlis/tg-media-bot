"""Tests for the audio upload flow (cover photo + audio with thumbnail)."""

import shutil
import subprocess
from unittest.mock import AsyncMock, MagicMock

import pytest

from src.services.uploader import UploaderService

ffmpeg_available = shutil.which("ffmpeg") is not None
requires_ffmpeg = pytest.mark.skipif(not ffmpeg_available, reason="ffmpeg not installed")


def _make_jpg(path, size="450x450"):
    subprocess.run(
        ["ffmpeg", "-y", "-f", "lavfi", "-i", f"color=c=blue:s={size}",
         "-frames:v", "1", str(path)],
        check=True, capture_output=True,
    )


@pytest.fixture
def bot():
    b = MagicMock()
    b.send_photo = AsyncMock(return_value=MagicMock(message_id=1))
    b.send_audio = AsyncMock(return_value=MagicMock(message_id=2))
    b.send_video = AsyncMock(return_value=MagicMock(message_id=3))
    return b


class TestUploadAudioCover:
    @requires_ffmpeg
    async def test_single_message_with_thumbnail_and_caption(self, bot, tmp_path):
        mp3 = tmp_path / "song.mp3"
        mp3.write_bytes(b"x" * 100)
        cover = tmp_path / "cover.jpg"
        _make_jpg(cover)

        up = UploaderService(bot)
        await up.upload_audio(
            chat_id=42, file_path=mp3, caption="Song", title="Song",
            performer="Artist", duration=100, thumbnail_path=cover,
            source_url="https://soundcloud.com/a/b",
        )

        # Telegram can't combine a photo + audio in one post: a single audio
        # message carries the album-art thumbnail and the title + URL caption.
        bot.send_photo.assert_not_awaited()
        bot.send_audio.assert_awaited_once()
        audio_kwargs = bot.send_audio.await_args.kwargs
        assert "<code>https://soundcloud.com/a/b</code>" in audio_kwargs["caption"]
        assert audio_kwargs["thumbnail"] is not None
        assert audio_kwargs["performer"] == "Artist"

    async def test_no_cover_still_has_caption(self, bot, tmp_path):
        mp3 = tmp_path / "song.mp3"
        mp3.write_bytes(b"x" * 100)

        up = UploaderService(bot)
        await up.upload_audio(
            chat_id=42, file_path=mp3, caption="Song", title="Song",
            thumbnail_path=None, source_url="https://x.com/y",
        )

        bot.send_photo.assert_not_awaited()
        bot.send_audio.assert_awaited_once()
        audio_kwargs = bot.send_audio.await_args.kwargs
        assert "<code>https://x.com/y</code>" in audio_kwargs["caption"]
        assert audio_kwargs["thumbnail"] is None


class TestUploadVideoThumbnail:
    @requires_ffmpeg
    async def test_video_sent_with_thumbnail_and_caption(self, bot, tmp_path):
        mp4 = tmp_path / "clip.mp4"
        mp4.write_bytes(b"x" * 100)
        poster = tmp_path / "poster.jpg"
        _make_jpg(poster, "1280x720")

        up = UploaderService(bot)
        await up.upload_video(
            chat_id=7, file_path=mp4, caption="Clip",
            source_url="https://youtu.be/abc", duration=120, thumbnail_path=poster,
        )

        bot.send_video.assert_awaited_once()
        kwargs = bot.send_video.await_args.kwargs
        assert kwargs["thumbnail"] is not None
        assert kwargs["duration"] == 120
        assert "<code>https://youtu.be/abc</code>" in kwargs["caption"]

    async def test_video_without_thumbnail(self, bot, tmp_path):
        mp4 = tmp_path / "clip.mp4"
        mp4.write_bytes(b"x" * 100)

        up = UploaderService(bot)
        await up.upload_video(chat_id=7, file_path=mp4, caption="Clip",
                              source_url="https://youtu.be/abc", thumbnail_path=None)

        bot.send_video.assert_awaited_once()
        assert bot.send_video.await_args.kwargs["thumbnail"] is None
