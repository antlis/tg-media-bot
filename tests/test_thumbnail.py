"""Tests for ffmpeg-based thumbnail preparation (Telegram limits)."""

import shutil
import subprocess

import pytest

from src.services.uploader import _prepare_thumbnail, _THUMB_MAX_BYTES

ffmpeg_available = shutil.which("ffmpeg") is not None
requires_ffmpeg = pytest.mark.skipif(not ffmpeg_available, reason="ffmpeg not installed")


def _make_jpg(path, size="450x450"):
    subprocess.run(
        ["ffmpeg", "-y", "-f", "lavfi", "-i", f"color=c=red:s={size}",
         "-frames:v", "1", str(path)],
        check=True, capture_output=True,
    )


class TestPrepareThumbnail:
    async def test_none_input(self):
        assert await _prepare_thumbnail(None) is None

    async def test_missing_file(self, tmp_path):
        assert await _prepare_thumbnail(tmp_path / "nope.jpg") is None

    @requires_ffmpeg
    async def test_resizes_within_telegram_limits(self, tmp_path):
        src = tmp_path / "cover.jpg"
        _make_jpg(src, "450x450")
        out = await _prepare_thumbnail(src)
        assert out is not None and out.exists()
        assert out.stat().st_size < _THUMB_MAX_BYTES

        # Dimensions must be <= 320 on each side
        dims = subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x", str(out)],
            capture_output=True, text=True,
        ).stdout.strip()
        w, h = (int(x) for x in dims.split("x"))
        assert w <= 320 and h <= 320

    @requires_ffmpeg
    async def test_landscape_aspect_preserved(self, tmp_path):
        src = tmp_path / "wide.jpg"
        _make_jpg(src, "640x360")
        out = await _prepare_thumbnail(src)
        assert out is not None
        dims = subprocess.run(
            ["ffprobe", "-v", "error", "-select_streams", "v:0",
             "-show_entries", "stream=width,height", "-of", "csv=p=0:s=x", str(out)],
            capture_output=True, text=True,
        ).stdout.strip()
        w, h = (int(x) for x in dims.split("x"))
        assert max(w, h) <= 320
        # 16:9 source → width should be the limiting dimension
        assert w >= h
