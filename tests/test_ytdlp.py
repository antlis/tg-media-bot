"""Tests for the yt-dlp downloader wrapper (no network)."""

import json

import pytest

from src.downloaders.ytdlp import (
    YtDlpDownloader,
    friendly_error,
    parse_progress_line,
    render_progress_bar,
)
from src.types.download import MediaFormat


class TestFriendlyError:
    @pytest.mark.parametrize("raw,needle", [
        ("ERROR: Sign in to confirm your age", "age-restricted"),
        ("ERROR: Private video. Sign in", "private"),
        ("ERROR: Requested format is not available", "quality"),
        ("ERROR: Video unavailable", "unavailable"),
        ("ERROR: Unsupported URL: https://x", "supported media URL"),
        ("ERROR: HTTP Error 404: Not Found", "404"),
        ("ERROR: blocked it in your country", "Region-restricted"),
    ])
    def test_maps_known_errors(self, raw, needle):
        assert needle.lower() in friendly_error(raw).lower()

    def test_unknown_error_trimmed(self):
        out = friendly_error("ERROR: some novel failure mode")
        assert "some novel failure mode" in out
        assert "ERROR:" not in out

    def test_empty(self):
        assert friendly_error("") == "Download failed."


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

    def test_video_recodes_for_inline_playback(self, dl, tmp_path):
        # Direct .webm URLs etc. must be transcoded so Telegram plays them inline
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.VIDEO)
        assert "--recode-video" in cmd
        assert "ffmpeg:-movflags +faststart" in cmd

    def test_auto_recodes_for_inline_playback(self, dl, tmp_path):
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO)
        assert "--recode-video" in cmd
        assert "ffmpeg:-movflags +faststart" in cmd

    def test_cookies_added_when_enabled(self, dl, tmp_path):
        dl.settings.use_browser_cookies = True
        dl.settings.browser_name = "firefox"
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO)
        assert "--cookies-from-browser" in cmd
        assert "firefox" in cmd

    def test_cookies_absent_when_disabled(self, dl, tmp_path):
        dl.settings.use_browser_cookies = False
        dl.settings.cookies_file = ""
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO)
        assert "--cookies-from-browser" not in cmd
        assert "--cookies" not in cmd

    def test_progress_flags_present(self, dl, tmp_path):
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUDIO)
        assert "--newline" in cmd and "--progress-template" in cmd
        assert any(arg.startswith("download:PROG|") for arg in cmd)

    def test_use_cookies_false_omits_cookies(self, dl, tmp_path):
        cookies = tmp_path / "cookies.txt"
        cookies.write_text("# Netscape HTTP Cookie File\n")
        dl.settings.cookies_file = str(cookies)
        dl.settings.use_browser_cookies = True
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO, use_cookies=False)
        assert "--cookies" not in cmd
        assert "--cookies-from-browser" not in cmd


class TestFormatErrorDetection:
    @pytest.mark.parametrize("err,expected", [
        ("ERROR: Requested format is not available", True),
        ("ERROR: Unable to extract player response", True),
        ("ERROR: no video formats found", True),
        ("ERROR: Private video", False),
        ("ERROR: HTTP Error 404", False),
        ("", False),
    ])
    def test_is_format_error(self, err, expected):
        assert YtDlpDownloader._is_format_error(err) is expected

    def test_cookies_file_used_when_present(self, dl, tmp_path):
        cookies = tmp_path / "cookies.txt"
        cookies.write_text("# Netscape HTTP Cookie File\n")
        dl.settings.cookies_file = str(cookies)
        dl.settings.use_browser_cookies = True  # file still wins
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO)
        assert "--cookies" in cmd
        assert str(cookies) in cmd
        assert "--cookies-from-browser" not in cmd

    def test_missing_cookies_file_falls_back_to_browser(self, dl, tmp_path):
        dl.settings.cookies_file = str(tmp_path / "does-not-exist.txt")
        dl.settings.use_browser_cookies = True
        cmd = dl._build_command("https://x", tmp_path, MediaFormat.AUTO)
        assert "--cookies" not in cmd
        assert "--cookies-from-browser" in cmd


class TestEffectiveFormat:
    def test_soundcloud_forced_to_audio(self, dl):
        assert dl._effective_format("soundcloud", MediaFormat.AUTO) == MediaFormat.AUDIO
        assert dl._effective_format("soundcloud", MediaFormat.VIDEO) == MediaFormat.AUDIO

    def test_youtube_respects_preference(self, dl):
        assert dl._effective_format("youtube", MediaFormat.AUTO) == MediaFormat.AUTO
        assert dl._effective_format("youtube", MediaFormat.VIDEO) == MediaFormat.VIDEO
        assert dl._effective_format("youtube", MediaFormat.AUDIO) == MediaFormat.AUDIO


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


class TestProgressParsing:
    def test_parses_full_line(self):
        info = parse_progress_line("PROG| 42.3%|1.20MiB/s|00:05")
        assert info["percent"] == 42.3
        assert info["percent_str"] == "42.3%"
        assert info["speed"] == "1.20MiB/s"
        assert info["eta"] == "00:05"

    def test_non_progress_line_is_none(self):
        assert parse_progress_line("[download] Destination: x.mp4") is None
        assert parse_progress_line("") is None

    def test_unknown_percent(self):
        info = parse_progress_line("PROG|   N/A|   N/A|   N/A")
        assert info is not None and info["percent"] is None

    @pytest.mark.parametrize("pct,filled", [(0, 0), (50, 5), (100, 10), (None, 0)])
    def test_bar_fill(self, pct, filled):
        bar = render_progress_bar(pct)
        assert bar.count("█") == filled
        assert len(bar) == 10

    def test_bar_clamps_over_100(self):
        assert render_progress_bar(150).count("█") == 10


class _FakeStream:
    def __init__(self, lines):
        self._lines = list(lines)

    async def readline(self):
        return self._lines.pop(0) if self._lines else b""


class _FakeProc:
    def __init__(self, stdout_lines, stderr_lines=None, returncode=0):
        self.stdout = _FakeStream(stdout_lines)
        self.stderr = _FakeStream(stderr_lines or [])
        self._rc = returncode
        self.returncode = None

    async def wait(self):
        self.returncode = self._rc
        return self._rc


class TestProgressStreaming:
    async def test_throttles_but_always_emits_completion(self, dl):
        # Three updates arrive back-to-back; the throttle should let the first
        # through and suppress the mid one, but 100% always fires.
        proc = _FakeProc([
            b"[download] Destination: x.mp4\n",   # ignored
            b"PROG| 10.0%|1.0MiB/s|00:10\n",      # first -> emit
            b"PROG| 50.0%|1.0MiB/s|00:05\n",      # within 3s -> suppressed
            b"PROG|100.0%|1.0MiB/s|00:00\n",      # done -> emit
        ])
        seen = []

        async def cb(info):
            seen.append(info["percent"])

        stderr = await dl._stream(proc, cb)
        assert seen == [10.0, 100.0]
        assert proc.returncode == 0
        assert stderr == ""

    async def test_no_callback_is_fine(self, dl):
        proc = _FakeProc([b"PROG| 50.0%|1.0MiB/s|00:05\n"], stderr_lines=[b"oops\n"])
        stderr = await dl._stream(proc, None)
        assert stderr == "oops\n"

    async def test_processing_stage_emitted_from_stderr(self, dl):
        proc = _FakeProc(
            stdout_lines=[b"PROG|100.0%|1.0MiB/s|00:00\n"],
            stderr_lines=[b"[Merger] Merging formats into \"x.mp4\"\n"],
        )
        stages = []

        async def cb(info):
            stages.append(info.get("stage"))

        await dl._stream(proc, cb)
        assert "download" in stages
        assert "process" in stages


class TestDownloadTimeout:
    async def test_timeout_kills_process_and_fails(self, dl, tmp_path, monkeypatch):
        import asyncio

        dl.settings.download_timeout = 0.01

        class BlockingStream:
            async def readline(self):
                await asyncio.sleep(10)  # never returns before the timeout
                return b""

        class FakeProc:
            def __init__(self):
                self.returncode = None
                self.killed = False
                self.stdout = BlockingStream()
                self.stderr = BlockingStream()

            def kill(self):
                self.killed = True
                self.returncode = -9

            async def wait(self):
                return self.returncode

        proc = FakeProc()

        async def fake_exec(*args, **kwargs):
            return proc

        monkeypatch.setattr("asyncio.create_subprocess_exec", fake_exec)

        result = await dl._run_download(["yt-dlp", "x"], tmp_path, "youtube")
        assert result.success is False
        assert "timed out" in result.error.lower()
        assert proc.killed is True


class TestCookieFallback:
    async def test_retries_without_cookies_on_format_error(self, dl, tmp_path, monkeypatch):
        from src.downloaders.ytdlp import DownloadResult
        cookies = tmp_path / "c.txt"
        cookies.write_text("# Netscape HTTP Cookie File\n")
        dl.settings.cookies_file = str(cookies)
        dl.settings.use_browser_cookies = False

        calls = []

        async def fake_run(cmd, output_dir, platform, cb=None):
            calls.append(cmd)
            if len(calls) == 1:
                return DownloadResult(success=False, platform=platform,
                                      error="ERROR: Requested format is not available")
            return DownloadResult(success=True, output_path=tmp_path / "x.mp4", platform=platform)

        monkeypatch.setattr(dl, "_run_download", fake_run)
        res = await dl.download("https://youtu.be/x", tmp_path, MediaFormat.AUTO)
        assert res.success is True
        assert len(calls) == 2
        assert "--cookies" in calls[0]       # first try used cookies
        assert "--cookies" not in calls[1]   # retry dropped them

    async def test_no_retry_without_cookies_configured(self, dl, tmp_path, monkeypatch):
        from src.downloaders.ytdlp import DownloadResult
        dl.settings.cookies_file = ""
        dl.settings.use_browser_cookies = False
        calls = []

        async def fake_run(cmd, output_dir, platform, cb=None):
            calls.append(cmd)
            return DownloadResult(success=False, platform=platform,
                                  error="ERROR: Requested format is not available")

        monkeypatch.setattr(dl, "_run_download", fake_run)
        res = await dl.download("https://youtu.be/x", tmp_path, MediaFormat.AUTO)
        assert res.success is False
        assert len(calls) == 1  # nothing to retry without


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
