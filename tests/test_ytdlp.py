"""Tests for the yt-dlp downloader wrapper (no network)."""

import json

import pytest

from src.downloaders.ytdlp import YtDlpDownloader
from src.types.download import MediaFormat


@pytest.fixture
def dl():
    # _check_yt_dlp is patched out by the autouse conftest fixture
    return YtDlpDownloader()


class TestDetectPlatform:
    @pytest.mark.parametrize("url,expected", [
        ("https://youtube.com/watch?v=x", "youtube"),
        ("https://youtu.be/x", "youtube"),
        ("https://soundcloud.com/a/b", "soundcloud"),
        ("https://vimeo.com/123", "vimeo"),
        ("https://www.tiktok.com/@a/video/1", "tiktok"),
        ("https://x.com/a/status/1", "twitter"),
        ("https://twitter.com/a/status/1", "twitter"),
        ("https://instagram.com/p/x", "instagram"),
        ("https://reddit.com/r/x", "reddit"),
        ("https://twitch.tv/x", "twitch"),
        ("https://example.com/x", "unknown"),
    ])
    def test_detect(self, dl, url, expected):
        assert dl.detect_platform(url) == expected


class TestValidateUrl:
    def test_valid(self, dl):
        assert dl.validate_url("https://youtube.com/watch?v=x")
        assert dl.validate_url("http://example.com")

    def test_invalid(self, dl):
        assert not dl.validate_url("")
        assert not dl.validate_url("ftp://example.com")
        assert not dl.validate_url("not a url")
        assert not dl.validate_url("https://" + "x" * 3000)


class TestBuildCommand:
    def test_audio_embeds_thumbnail_and_metadata(self, dl, tmp_path):
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUDIO)
        for flag in ("-x", "--embed-thumbnail", "--embed-metadata", "--write-info-json"):
            assert flag in cmd
        assert "mp3" in cmd

    def test_video_merges_to_mp4(self, dl, tmp_path):
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.VIDEO)
        assert "--merge-output-format" in cmd
        assert "mp4" in cmd

    def test_cookies_added_when_enabled(self, dl, tmp_path):
        dl.settings.use_browser_cookies = True
        dl.settings.browser_name = "firefox"
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO)
        assert "--cookies-from-browser" in cmd
        assert "firefox" in cmd

    def test_cookies_absent_when_disabled(self, dl, tmp_path):
        dl.settings.use_browser_cookies = False
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO)
        assert "--cookies-from-browser" not in cmd


class TestFileDiscovery:
    def test_find_downloaded_file_picks_largest_media(self, dl, tmp_path):
        (tmp_path / "small.mp3").write_bytes(b"x" * 10)
        (tmp_path / "big.mp4").write_bytes(b"x" * 100)
        (tmp_path / "cover.jpg").write_bytes(b"x" * 9999)  # not media
        assert dl._find_downloaded_file(tmp_path).name == "big.mp4"

    def test_find_downloaded_file_none_when_empty(self, dl, tmp_path):
        assert dl._find_downloaded_file(tmp_path) is None

    def test_find_thumbnail_prefers_jpg(self, dl, tmp_path):
        (tmp_path / "a.png").write_bytes(b"x")
        (tmp_path / "a.jpg").write_bytes(b"x")
        assert dl._find_thumbnail(tmp_path).suffix == ".jpg"

    def test_find_thumbnail_none(self, dl, tmp_path):
        (tmp_path / "only.mp3").write_bytes(b"x")
        assert dl._find_thumbnail(tmp_path) is None

    def test_read_info_json(self, dl, tmp_path):
        data = {"title": "T", "artist": "A", "duration": 100}
        (tmp_path / "song.info.json").write_text(json.dumps(data))
        assert dl._read_info_json(tmp_path)["artist"] == "A"

    def test_read_info_json_missing(self, dl, tmp_path):
        assert dl._read_info_json(tmp_path) == {}

    def test_read_info_json_invalid(self, dl, tmp_path):
        (tmp_path / "bad.info.json").write_text("{not json")
        assert dl._read_info_json(tmp_path) == {}


class TestFormatsList:
    def test_empty(self, dl):
        assert dl.format_formats_list([]) == "No formats available."

    def test_lists_entries(self, dl):
        formats = [{"format_id": "18", "ext": "mp4", "type": "video+audio",
                    "resolution": "360p", "filesize": 6_500_000}]
        out = dl.format_formats_list(formats)
        assert "18" in out and "mp4" in out and "6.2MB" in out


class TestGetFormats:
    async def test_parses_info(self, dl, monkeypatch):
        async def fake_info(url):
            return {"formats": [
                {"format_id": "140", "ext": "m4a", "vcodec": "none", "acodec": "mp4a"},
                {"format_id": "137", "ext": "mp4", "vcodec": "avc1", "acodec": "none"},
            ]}
        monkeypatch.setattr(dl, "get_info", fake_info)
        formats = await dl.get_formats("https://x")
        types = {f["format_id"]: f["type"] for f in formats}
        assert types["140"] == "audio"
        assert types["137"] == "video"

    async def test_empty_when_no_info(self, dl, monkeypatch):
        async def fake_info(url):
            return None
        monkeypatch.setattr(dl, "get_info", fake_info)
        assert await dl.get_formats("https://x") == []
