"""yt-dlp downloader implementation."""

import asyncio
import json
import re
import shutil
import subprocess
import tempfile
import time
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, List, Optional, Tuple

from ..config import get_settings
from ..types.download import MediaFormat
from ..utils.logger import get_logger

logger = get_logger()

# Per-connection network stall timeout (seconds). Distinct from the total
# DOWNLOAD_TIMEOUT, which bounds the whole yt-dlp run.
_SOCKET_TIMEOUT = 60

# Extra attempts for a transient connectivity error before giving up (or
# falling back to a proxy). Each attempt is a fresh yt-dlp process, so it
# re-resolves DNS from scratch — useful when a site resolves to a pool of
# several IPs and only some of them are actually reachable.
_CONNECTIVITY_RETRY_ATTEMPTS = 3

# Live-progress wiring. yt-dlp prints one PROG line per update (--newline +
# --progress-template); we parse it and throttle callbacks so message edits
# stay well under Telegram's rate limit.
_PROGRESS_PREFIX = "PROG|"
_PROGRESS_TEMPLATE = (
    "download:" + _PROGRESS_PREFIX
    + "%(progress._percent_str)s|%(progress._speed_str)s|%(progress._eta_str)s"
)
_PROGRESS_MIN_INTERVAL = 3.0  # seconds between progress callbacks
_PCT_RE = re.compile(r"([\d.]+)%")

# yt-dlp stderr markers that mean it has finished downloading and is now
# post-processing (merging streams, recoding, embedding art) — no % for these.
_POSTPROCESS_MARKERS = (
    "[Merger]", "[VideoConvertor]", "[ExtractAudio]", "[Recode",
    "[EmbedThumbnail]", "[Metadata]", "[ThumbnailsConvertor]",
    "Merging formats", "Converting",
)


def parse_progress_line(line: str) -> Optional[dict]:
    """Parse a yt-dlp PROG line into {percent, percent_str, speed, eta}.

    Returns None for any line that isn't a progress update.
    """
    if not line.startswith(_PROGRESS_PREFIX):
        return None
    parts = line.split("|")
    pct_str = parts[1].strip() if len(parts) > 1 else ""
    speed = parts[2].strip() if len(parts) > 2 else ""
    eta = parts[3].strip() if len(parts) > 3 else ""
    m = _PCT_RE.search(pct_str)
    percent = float(m.group(1)) if m else None
    return {"percent": percent, "percent_str": pct_str, "speed": speed, "eta": eta}


def render_progress_bar(percent: Optional[float], width: int = 10) -> str:
    """Render a ``█/░`` bar for a 0–100 percentage (None → empty bar)."""
    pct = 0.0 if percent is None else max(0.0, min(100.0, percent))
    filled = int(pct / 100 * width)
    return "█" * filled + "░" * (width - filled)


# Substring (lowercased) → friendly, actionable hint. First match wins.
_ERROR_HINTS = (
    ("sign in to confirm your age", "🔞 Age-restricted — set a COOKIES_FILE from a logged-in account."),
    ("confirm you're not a bot", "🤖 This site wants a logged-in session — set a COOKIES_FILE."),
    ("private video", "🔒 This video is private."),
    ("members-only", "🔒 Members-only content — needs a subscribed account's cookies."),
    ("login required", "🔑 Login required — set a COOKIES_FILE from a logged-in account."),
    ("only available for registered", "🔑 Registered users only — set a COOKIES_FILE."),
    ("requested format is not available", "🎚️ That quality isn't available — try another via /formats."),
    ("video unavailable", "🚫 The video is unavailable (removed or not public)."),
    ("unsupported url", "🤔 That link isn't a supported media URL."),
    ("unable to extract", "🤔 Couldn't read that page — the link may be wrong or unsupported."),
    ("http error 404", "🚫 Not found (404) — the link may be dead."),
    ("http error 403", "⛔ Access denied (403) — may need cookies or a different region."),
    ("unable to download webpage", "🌐 Network hiccup reaching the site — try again."),
)


def friendly_error(raw: str) -> str:
    """Map a raw yt-dlp error to a short, actionable hint.

    Falls back to a trimmed version of the raw error when nothing matches.
    """
    e = (raw or "").lower()
    for needle, hint in _ERROR_HINTS:
        if needle in e:
            return hint
    if "geo" in e or "in your country" in e or "in your region" in e:
        return "🌍 Region-restricted — set a PROXY_URL to retry through another region."
    cleaned = (raw or "").replace("ERROR:", "").strip()
    return cleaned[:300] if cleaned else "Download failed."


@dataclass
class DownloadResult:
    """Result of a download operation."""

    success: bool
    output_path: Optional[Path] = None
    file_size: int = 0
    title: str = ""
    platform: str = ""
    duration: float = 0.0
    error: str = ""
    performer: str = ""
    thumbnail_path: Optional[Path] = None


class YtDlpDownloader:
    """
    yt-dlp-based downloader for media from various platforms.

    Supports:
    - YouTube, SoundCloud, Vimeo, TikTok, Twitter/X, Instagram, Reddit, Twitch
    - Video and audio-only downloads
    - Firefox cookie authentication
    - Progress tracking
    """

    # Platform detection patterns
    PLATFORM_PATTERNS = {
        "youtube": r"(?:youtube\.com|youtu\.be)",
        "soundcloud": r"soundcloud\.com",
        "vimeo": r"vimeo\.com",
        "tiktok": r"tiktok\.com",
        "twitter": r"(?:twitter\.com|x\.com)",
        "instagram": r"instagram\.com",
        "reddit": r"reddit\.com",
        "twitch": r"twitch\.tv",
    }

    # Platforms that only ever serve audio — always download as audio so the
    # result is a proper tagged MP3 with cover art, even without /audio mode.
    AUDIO_ONLY_PLATFORMS = {"soundcloud"}

    def __init__(self):
        self.settings = get_settings()
        self._check_yt_dlp()

    def _check_yt_dlp(self):
        """Verify yt-dlp is available."""
        try:
            result = subprocess.run(
                ["yt-dlp", "--version"],
                capture_output=True,
                text=True,
                check=True,
            )
            logger.info(f"yt-dlp version: {result.stdout.strip()}")
        except (subprocess.SubprocessError, FileNotFoundError) as e:
            raise RuntimeError(f"yt-dlp not found or not working: {e}")

    def _cookie_args(self) -> List[str]:
        """yt-dlp cookie flags. A cookies.txt (if present) wins over browser
        cookies; in Docker the file is the only option (no browser)."""
        cookies_file = self.settings.cookies_file
        if cookies_file and Path(cookies_file).exists():
            return ["--cookies", cookies_file]
        if self.settings.use_browser_cookies:
            return ["--cookies-from-browser", self.settings.browser_name]
        return []

    def detect_platform(self, url: str) -> str:
        """Detect platform from URL."""
        for platform, pattern in self.PLATFORM_PATTERNS.items():
            if re.search(pattern, url, re.IGNORECASE):
                return platform
        return "unknown"

    def validate_url(self, url: str) -> bool:
        """Validate if URL is a supported media URL."""
        if not url or not isinstance(url, str):
            return False

        url = url.strip()
        if not url.startswith(("http://", "https://")):
            return False

        # Check length
        if len(url) > 2048:
            return False

        return True

    async def get_info(self, url: str) -> Optional[dict]:
        """
        Get video info without downloading.

        Returns:
            Dict with video metadata or None on failure.
        """
        cmd = [
            "yt-dlp",
            "--dump-json",
            "--no-download",
            "--no-playlist",
        ]

        cmd.extend(self._cookie_args())

        cmd.append(url)

        result = None
        try:
            result = await asyncio.create_subprocess_exec(
                *cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )
            stdout, stderr = await asyncio.wait_for(
                result.communicate(), timeout=self.settings.download_timeout
            )

            if result.returncode != 0:
                logger.debug(f"get_info failed: {stderr.decode()[:200]}")
                return None

            return json.loads(stdout.decode())

        except asyncio.TimeoutError:
            await self._terminate(result)
            logger.debug("get_info timed out")
            return None
        except (asyncio.SubprocessError, json.JSONDecodeError) as e:
            logger.debug(f"get_info error: {e}")
            return None

    async def get_formats(self, url: str) -> List[dict]:
        """
        Get available formats for a URL.

        Returns:
            List of format dicts with id, ext, resolution, etc.
        """
        info = await self.get_info(url)
        if not info:
            return []

        formats = info.get("formats", [])
        result = []

        for f in formats:
            format_id = f.get("format_id", "")
            ext = f.get("ext", "")
            format_note = f.get("format_note", "")
            resolution = f.get("resolution", "")

            # Determine type
            if f.get("vcodec") == "none" or not f.get("vcodec"):
                media_type = "audio"
            elif f.get("acodec") == "none" or not f.get("acodec"):
                media_type = "video"
            else:
                media_type = "video+audio"

            result.append({
                "format_id": format_id,
                "ext": ext,
                "type": media_type,
                "resolution": resolution or format_note,
                "filesize": f.get("filesize") or f.get("filesize_approx", 0),
            })

        return result

    async def download(
        self,
        url: str,
        output_dir: Path,
        preferred_format: MediaFormat = MediaFormat.AUTO,
        progress_callback: Optional[callable] = None,
        max_height: Optional[int] = None,
    ) -> DownloadResult:
        """
        Download media from URL.

        Args:
            url: Media URL
            output_dir: Directory to save downloaded file
            preferred_format: Preferred media format (video/audio/auto)
            progress_callback: Optional callback for progress updates
            max_height: Optional max video height (e.g. 720) from the picker

        Returns:
            DownloadResult with success status and file info.
        """
        if not self.validate_url(url):
            return DownloadResult(success=False, error="Invalid URL")

        platform = self.detect_platform(url)
        effective_format = self._effective_format(platform, preferred_format)
        logger.info(f"Starting download", platform=platform, url=url[:80])

        # Build and run.
        cmd = self._build_command(url, output_dir, effective_format, max_height=max_height)
        result = await self._run_download(cmd, output_dir, platform, progress_callback)

        # Fallback: cookies can break extraction on some sites (e.g. a flagged
        # YouTube session forces a format-less "tv downgraded" response). If a
        # format/extraction error came back and cookies were in play, retry
        # once without them — but keep the original error if the retry also fails.
        if (
            not result.success
            and self._cookie_args()
            and self._is_format_error(result.error)
        ):
            logger.info(
                "Format error with cookies — retrying without cookies", platform=platform
            )
            cmd = self._build_command(
                url, output_dir, effective_format, max_height=max_height, use_cookies=False
            )
            retry = await self._run_download(cmd, output_dir, platform, progress_callback)
            if retry.success:
                result = retry

        # Transient connectivity failures (a handshake stall, a reset, a
        # momentary drop) often clear up on their own — a site can resolve to
        # a pool of several IPs where only some are actually reachable, and
        # each fresh attempt re-resolves DNS and gets an independent shot at
        # a working one. Worth a few bare retries on the same path before
        # reaching for a proxy.
        attempt = 0
        while (
            not result.success
            and self._is_connectivity_error(result.error)
            and attempt < _CONNECTIVITY_RETRY_ATTEMPTS
        ):
            attempt += 1
            logger.info(
                f"Transient connectivity error — retrying ({attempt}/{_CONNECTIVITY_RETRY_ATTEMPTS})",
                platform=platform,
            )
            result = await self._run_download(cmd, output_dir, platform, progress_callback)

        if (
            not result.success
            and self.settings.proxy_url
            and self._is_proxy_worthy(result.error)
        ):
            logger.info(
                "Geo-restriction or connectivity block detected — retrying via proxy",
                platform=platform,
            )
            cmd = self._build_command(
                url, output_dir, effective_format, proxy=self.settings.proxy_url,
                max_height=max_height,
            )
            result = await self._run_download(cmd, output_dir, platform, progress_callback)

        return result

    # Substrings in yt-dlp stderr that indicate a country/region licensing block.
    _GEO_MARKERS = (
        "geo restriction",
        "geo-restricted",
        "not available from your location",
        "not available in your country",
        "available in your country",
        "in your region",
        "blocked it in your country",
    )

    # Substrings indicating the connection itself was blocked/dropped (ISP
    # filtering, anti-bot IP blocks, etc.) rather than a normal transient
    # network hiccup — worth one retry through the proxy, same as a geo-block.
    _CONNECTIVITY_MARKERS = (
        "handshake operation timed out",
        "connection refused",
        "network is unreachable",
        "unable to download webpage",
        "unable to connect to proxy",
        "connection reset by peer",
    )

    def _is_geo_blocked(self, error: str) -> bool:
        """True if the failure looks like a country/region licensing block."""
        e = (error or "").lower()
        return any(m in e for m in self._GEO_MARKERS)

    def _is_connectivity_error(self, error: str) -> bool:
        """True if the failure looks like a transient connection/handshake
        problem (as opposed to a definitive geo/content block) — worth a bare
        retry on the same path before trying a proxy."""
        e = (error or "").lower()
        return any(m in e for m in self._CONNECTIVITY_MARKERS)

    def _is_proxy_worthy(self, error: str) -> bool:
        """True if a proxied retry is likely to help: an explicit geo-block,
        or the connection to the site itself being blocked/dropped."""
        return self._is_geo_blocked(error) or self._is_connectivity_error(error)

    @staticmethod
    def _is_format_error(error: str) -> bool:
        """True if the failure is a format/extraction problem cookies might cause."""
        e = (error or "").lower()
        return (
            "requested format is not available" in e
            or "unable to extract" in e
            or "no video formats" in e
        )

    @staticmethod
    async def _terminate(process) -> None:
        """Kill a still-running subprocess and reap it (best effort)."""
        if process and process.returncode is None:
            try:
                process.kill()
                await process.wait()
            except ProcessLookupError:
                pass

    async def _stream(self, process, progress_callback: Optional[Callable]) -> str:
        """Read the subprocess to completion, forwarding throttled progress.

        Returns the collected stderr text. stdout PROG lines drive
        ``progress_callback`` (at most once per ``_PROGRESS_MIN_INTERVAL``,
        always firing on 100%); other stdout lines are discarded.
        """
        stderr_chunks: List[str] = []
        emit_lock = asyncio.Lock()
        last_emit = 0.0

        async def emit(info: dict, force: bool = False) -> None:
            nonlocal last_emit
            if not progress_callback:
                return
            if not force and time.monotonic() - last_emit < _PROGRESS_MIN_INTERVAL:
                return
            async with emit_lock:
                last_emit = time.monotonic()
                try:
                    await progress_callback(info)
                except Exception as e:  # a failed edit must not break the download
                    logger.debug(f"progress callback error: {e}")

        announced_processing = False

        async def drain_stderr():
            nonlocal announced_processing
            while True:
                line = await process.stderr.readline()
                if not line:
                    break
                text = line.decode(errors="replace")
                stderr_chunks.append(text)
                # Surface the post-download processing phase once, so the bar
                # doesn't sit at a confusing 100% during the merge/recode.
                if not announced_processing and any(m in text for m in _POSTPROCESS_MARKERS):
                    announced_processing = True
                    await emit({"stage": "process"}, force=True)

        stderr_task = asyncio.ensure_future(drain_stderr())
        try:
            while True:
                raw = await process.stdout.readline()
                if not raw:
                    break
                info = parse_progress_line(raw.decode(errors="replace").strip())
                if info is None:
                    continue
                info["stage"] = "download"
                done = info["percent"] is not None and info["percent"] >= 100
                await emit(info, force=done)
        except asyncio.CancelledError:
            # Timeout/cancel: don't await the process here (it isn't dead yet —
            # the caller kills it). Just stop draining stderr and propagate.
            stderr_task.cancel()
            raise

        # Normal completion: stdout closed, so the process is finishing.
        await process.wait()
        await stderr_task
        return "".join(stderr_chunks)

    async def _run_download(
        self,
        cmd: List[str],
        output_dir: Path,
        platform: str,
        progress_callback: Optional[Callable] = None,
    ) -> DownloadResult:
        """Run a single yt-dlp invocation and parse the result.

        Streams download progress to ``progress_callback``. Bounded by
        ``DOWNLOAD_TIMEOUT``; on timeout or cancellation the child process is
        killed so it can't keep downloading in the background.
        """
        process = None
        try:
            process = await asyncio.create_subprocess_exec(
                *cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )

            try:
                stderr = await asyncio.wait_for(
                    self._stream(process, progress_callback),
                    timeout=self.settings.download_timeout,
                )
            except asyncio.TimeoutError:
                await self._terminate(process)
                logger.error(
                    f"Download timed out after {self.settings.download_timeout}s"
                )
                return DownloadResult(
                    success=False,
                    error=f"Download timed out after {self.settings.download_timeout}s",
                    platform=platform,
                )

            if process.returncode != 0:
                error_msg = stderr.strip()
                # Try to extract useful error message
                if "ERROR" in error_msg:
                    error_lines = [l for l in error_msg.split("\n") if "ERROR" in l]
                    if error_lines:
                        error_msg = error_lines[-1]
                else:
                    error_msg = error_msg[:200] if error_msg else "Unknown error"

                logger.error(f"Download failed: {error_msg}")
                return DownloadResult(success=False, error=error_msg, platform=platform)

            # Find downloaded file
            output_file = self._find_downloaded_file(output_dir)
            if not output_file:
                return DownloadResult(
                    success=False,
                    error="Download completed but output file not found",
                    platform=platform,
                )

            file_size = output_file.stat().st_size

            # Collect cover art + metadata (audio downloads write these)
            thumbnail_file = self._find_thumbnail(output_dir)
            meta = self._read_info_json(output_dir)
            title = meta.get("track") or meta.get("title") or output_file.stem
            performer = meta.get("artist") or meta.get("uploader") or ""
            try:
                duration = float(meta.get("duration") or 0.0)
            except (TypeError, ValueError):
                duration = 0.0

            return DownloadResult(
                success=True,
                output_path=output_file,
                file_size=file_size,
                title=title,
                performer=performer,
                duration=duration,
                thumbnail_path=thumbnail_file,
                platform=platform,
            )

        except asyncio.CancelledError:
            # User cancelled (/cancel) — stop the child process, then propagate.
            await self._terminate(process)
            raise
        except Exception as e:
            logger.error(f"Download exception: {e}")
            await self._terminate(process)
            return DownloadResult(
                success=False,
                error=str(e)[:200],
                platform=platform,
            )

    def _effective_format(self, platform: str, preferred: MediaFormat) -> MediaFormat:
        """Resolve the format to actually download.

        Audio-only platforms (e.g. SoundCloud) are always fetched as audio so
        the result is a tagged MP3 with cover art, regardless of the user's
        video/auto preference.
        """
        if platform in self.AUDIO_ONLY_PLATFORMS:
            return MediaFormat.AUDIO
        return preferred

    @staticmethod
    def _video_selector(max_height: Optional[int]) -> str:
        """yt-dlp -f selector for video, optionally capped to a max height."""
        if max_height:
            h = f"[height<=?{max_height}]"
            return (
                f"bestvideo{h}[ext=mp4]+bestaudio[ext=m4a]/"
                f"bestvideo{h}+bestaudio/best{h}/best"
            )
        return "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo[ext=webm]+bestaudio[ext=webm]/18/best"

    def _build_command(
        self,
        url: str,
        output_dir: Path,
        preferred_format: MediaFormat,
        proxy: Optional[str] = None,
        max_height: Optional[int] = None,
        use_cookies: bool = True,
    ) -> List[str]:
        """Build yt-dlp command with appropriate options."""
        # --restrict-filenames: ASCII-only, no spaces/special chars in the
        # %(title)s-derived output filename. Some titles contain characters
        # (e.g. a literal "%") that make ffmpeg's own file writer choke with
        # "Error opening output files: Invalid argument" during the
        # --recode-video/--movflags postprocessing step.
        cmd = ["yt-dlp", "--no-playlist", "--restrict-filenames"]

        # Route through a proxy (used only for the geo-restriction fallback retry)
        if proxy:
            cmd.extend(["--proxy", proxy])

        # Cookie authentication (skipped on the no-cookies fallback retry)
        if use_cookies:
            cmd.extend(self._cookie_args())

        # Suppress version update warning
        cmd.append("--no-update")

        # Output template
        output_template = str(output_dir / "%(title)s.%(ext)s")
        cmd.extend(["-o", output_template])

        # Format selection based on preference
        if preferred_format == MediaFormat.AUDIO:
            cmd.extend([
                "-x",
                "--audio-format", "mp3",
                "--audio-quality", "0",
                # Bake cover art + tags into the mp3 itself
                "--embed-thumbnail",
                "--embed-metadata",
                # Sidecar metadata so we can read title/artist/duration without
                # a second network call (used to set the Telegram audio fields)
                "--write-info-json",
            ])
        else:
            # VIDEO and AUTO behave identically: best video+audio, merged to mp4,
            # with a fallback to format 18. Direct-file URLs (e.g. .webm on
            # imageboards) bypass the format selector — yt-dlp downloads the raw
            # file — so re-encode anything non-mp4 to H.264/AAC and put the moov
            # atom up front, letting Telegram play it inline rather than as a
            # downloadable file.
            cmd.extend([
                "-f", self._video_selector(max_height),
                "--merge-output-format", "mp4",
                "--recode-video", "mp4",
                # Scoped to the video-recode postprocessor specifically — a
                # bare "ffmpeg:" key applies to *every* ffmpeg postprocessor
                # yt-dlp runs, including the unrelated thumbnail-to-jpg
                # conversion below, where an mp4-only muxer flag like
                # -movflags has no business being and can make it choke.
                "--postprocessor-args", "VideoConvertor:-movflags +faststart",
            ])

        # Write the raw thumbnail as-is. Deliberately NOT using
        # --convert-thumbnails: yt-dlp's own converter unconditionally forces
        # ffmpeg's "-f image2" demuxer on the input, which can't parse AVIF
        # thumbnails (an ISOBMFF/AV1 container, not a raw image2 bitstream)
        # and fails with "Error opening output files: Invalid argument".
        # _prepare_thumbnail() in uploader.py does its own plain, format-
        # agnostic ffmpeg conversion downstream instead.
        cmd.append("--write-thumbnail")

        # Per-connection network stall timeout
        cmd.extend([
            "--socket-timeout", str(_SOCKET_TIMEOUT),
        ])

        # Don't overwrite
        cmd.append("--no-overwrites")

        # Emit machine-parseable download progress, one update per line, so the
        # runner can stream it back as a live bar.
        cmd.extend([
            "--no-warnings",
            "--progress",
            "--newline",
            "--progress-template", _PROGRESS_TEMPLATE,
        ])

        # URL
        cmd.append(url)

        return cmd

    def _find_downloaded_file(self, directory: Path) -> Optional[Path]:
        """Find the main downloaded media file in directory."""
        if not directory.exists():
            return None

        # Find media files (exclude thumbnails)
        media_extensions = {".mp4", ".mkv", ".webm", ".mp3", ".m4a", ".wav", ".flac", ".ogg"}

        files = []
        for f in directory.iterdir():
            if f.is_file() and f.suffix.lower() in media_extensions:
                files.append(f)

        if not files:
            return None

        # Return largest file (main content)
        return max(files, key=lambda f: f.stat().st_size)

    def _find_thumbnail(self, directory: Path) -> Optional[Path]:
        """Find the downloaded cover-art image, if any."""
        if not directory.exists():
            return None

        thumb_extensions = (".jpg", ".jpeg", ".png", ".webp", ".avif")
        thumbs = [
            f for f in directory.iterdir()
            if f.is_file() and f.suffix.lower() in thumb_extensions
        ]
        if not thumbs:
            return None

        # Prefer jpg when there's a choice; _prepare_thumbnail() converts
        # whatever we hand it (raw source format, since we don't ask yt-dlp
        # to convert — see _build_command).
        thumbs.sort(key=lambda f: (f.suffix.lower() != ".jpg", f.name))
        return thumbs[0]

    def _read_info_json(self, directory: Path) -> dict:
        """Read the yt-dlp .info.json sidecar, if present."""
        if not directory.exists():
            return {}

        for f in directory.iterdir():
            if f.is_file() and f.name.endswith(".info.json"):
                try:
                    return json.loads(f.read_text(encoding="utf-8"))
                except (OSError, json.JSONDecodeError) as e:
                    logger.debug(f"Could not read info json: {e}")
                    return {}
        return {}

    def format_formats_list(self, formats: List[dict]) -> str:
        """Format formats list for display."""
        if not formats:
            return "No formats available."

        lines = ["Available formats:\n"]

        for i, fmt in enumerate(formats[:20], 1):  # Limit to 20
            fmt_id = fmt.get("format_id", "")
            ext = fmt.get("ext", "")
            fmt_type = fmt.get("type", "")
            res = fmt.get("resolution", "")
            size = fmt.get("filesize", 0)

            size_str = ""
            if size:
                size_str = f" ({size / (1024*1024):.1f}MB)"

            lines.append(f"  {i}. [{fmt_id}] {fmt_type} {ext} {res}{size_str}")

        if len(formats) > 20:
            lines.append(f"\n  ... and {len(formats) - 20} more formats")

        return "\n".join(lines)


# Convenience function
async def download_media(
    url: str,
    output_dir: Path,
    preferred_format: MediaFormat = MediaFormat.AUTO,
) -> DownloadResult:
    """Download media using default downloader."""
    downloader = YtDlpDownloader()
    return await downloader.download(url, output_dir, preferred_format)
