"""yt-dlp downloader implementation."""

import asyncio
import json
import re
import shutil
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import List, Optional, Tuple

from ..config import get_settings
from ..types.download import MediaFormat
from ..utils.logger import get_logger
from ..utils.sanitizer import sanitize_filename

logger = get_logger()

# Per-connection network stall timeout (seconds). Distinct from the total
# DOWNLOAD_TIMEOUT, which bounds the whole yt-dlp run.
_SOCKET_TIMEOUT = 60


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
    ) -> DownloadResult:
        """
        Download media from URL.

        Args:
            url: Media URL
            output_dir: Directory to save downloaded file
            preferred_format: Preferred media format (video/audio/auto)
            progress_callback: Optional callback for progress updates

        Returns:
            DownloadResult with success status and file info.
        """
        if not self.validate_url(url):
            return DownloadResult(success=False, error="Invalid URL")

        platform = self.detect_platform(url)
        effective_format = self._effective_format(platform, preferred_format)
        logger.info(f"Starting download", platform=platform, url=url[:80])

        # Build and run; on a geo-restriction failure, retry once via the proxy.
        cmd = self._build_command(url, output_dir, effective_format)
        result = await self._run_download(cmd, output_dir, platform)

        if (
            not result.success
            and self.settings.proxy_url
            and self._is_geo_blocked(result.error)
        ):
            logger.info(
                "Geo-restriction detected — retrying via proxy", platform=platform
            )
            cmd = self._build_command(
                url, output_dir, effective_format, proxy=self.settings.proxy_url
            )
            result = await self._run_download(cmd, output_dir, platform)

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

    def _is_geo_blocked(self, error: str) -> bool:
        """True if the failure looks like a country/region licensing block."""
        e = (error or "").lower()
        return any(m in e for m in self._GEO_MARKERS)

    @staticmethod
    async def _terminate(process) -> None:
        """Kill a still-running subprocess and reap it (best effort)."""
        if process and process.returncode is None:
            try:
                process.kill()
                await process.wait()
            except ProcessLookupError:
                pass

    async def _run_download(
        self, cmd: List[str], output_dir: Path, platform: str
    ) -> DownloadResult:
        """Run a single yt-dlp invocation and parse the result.

        Bounded by ``DOWNLOAD_TIMEOUT``; on timeout or cancellation the child
        process is killed so it can't keep downloading in the background.
        """
        process = None
        try:
            process = await asyncio.create_subprocess_exec(
                *cmd,
                stdout=asyncio.subprocess.PIPE,
                stderr=asyncio.subprocess.PIPE,
            )

            try:
                stdout, stderr = await asyncio.wait_for(
                    process.communicate(),
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
                error_msg = stderr.decode(errors="replace").strip()
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

    def _build_command(
        self,
        url: str,
        output_dir: Path,
        preferred_format: MediaFormat,
        proxy: Optional[str] = None,
    ) -> List[str]:
        """Build yt-dlp command with appropriate options."""
        cmd = ["yt-dlp", "--no-playlist"]

        # Route through a proxy (used only for the geo-restriction fallback retry)
        if proxy:
            cmd.extend(["--proxy", proxy])

        # Cookie authentication
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
                "-f", "bestvideo[ext=mp4]+bestaudio[ext=m4a]/bestvideo[ext=webm]+bestaudio[ext=webm]/18/best",
                "--merge-output-format", "mp4",
                "--recode-video", "mp4",
                "--postprocessor-args", "ffmpeg:-movflags +faststart",
            ])

        # Post-processing for thumbnails
        cmd.extend([
            "--write-thumbnail",
            "--convert-thumbnails", "jpg",
        ])

        # Per-connection network stall timeout
        cmd.extend([
            "--socket-timeout", str(_SOCKET_TIMEOUT),
        ])

        # Don't overwrite
        cmd.append("--no-overwrites")

        # Quiet for cleaner output
        cmd.append("-q")

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

        thumb_extensions = (".jpg", ".jpeg", ".png", ".webp")
        thumbs = [
            f for f in directory.iterdir()
            if f.is_file() and f.suffix.lower() in thumb_extensions
        ]
        if not thumbs:
            return None

        # Prefer jpg (yt-dlp converts to jpg via --convert-thumbnails)
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
