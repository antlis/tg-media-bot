"""Headless-browser fallback for pages whose player builds the media URL in
JavaScript.

yt-dlp only parses the static HTML a page ships. Some sites expose no stream
there — the player fetches (or assembles) the media URL at runtime via JS — so
every extractor, generic included, gives up with "Unsupported URL". This module
loads such a page in headless Chromium, watches network traffic for the media
request the player makes, and returns that URL for yt-dlp to download normally.

Playwright + Chromium are an optional dependency: if they're not installed the
resolver quietly returns ``None`` and the caller just reports the original
failure.
"""

import re
from typing import Optional, Tuple

from ..utils.logger import get_logger

logger = get_logger()

# Requests that carry (or point at) playable media.
_MEDIA_RE = re.compile(
    r"\.(m3u8|mpd|mp4|m4s|ts|webm)(\?|$)|/get_file/|videoplayback|mediadelivery|videodelivery",
    re.I,
)
# Junk we never want to pick even if it matches: previews, trailers, thumbs, ads.
_JUNK_RE = re.compile(r"preview|trailer|sample|/thumb|_tr\.|sprite|/ads?/", re.I)

# Play-button selectors to click — many players only request the stream on play.
_PLAY_SELECTORS = (
    "video", ".jwplayer", ".jw-icon-display", "[aria-label='Play']",
    ".vjs-big-play-button", "#player", ".play-button",
)

_UA = (
    "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 "
    "(KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36"
)


def _score(url: str) -> int:
    """Rank captured URLs: manifests beat progressive files; junk sinks."""
    s = 0
    if re.search(r"\.(m3u8|mpd)(\?|$)", url, re.I):
        s += 100          # a manifest lets yt-dlp choose the best quality itself
    elif re.search(r"\.(mp4|webm|m4s|ts)(\?|$)|/get_file/", url, re.I):
        s += 50
    if _JUNK_RE.search(url):
        s -= 200
    return s


async def resolve_media_url(
    page_url: str, timeout: int = 45
) -> Optional[Tuple[str, str]]:
    """Return ``(media_url, referer)`` sniffed from the page's player, or None.

    ``referer`` is the page URL — most CDNs require it when yt-dlp refetches the
    captured stream.
    """
    try:
        from playwright.async_api import async_playwright
    except ImportError:
        logger.warning("Playwright not installed — skipping headless-browser fallback")
        return None

    captured: list[str] = []
    try:
        async with async_playwright() as p:
            browser = await p.chromium.launch(headless=True, args=["--no-sandbox"])
            ctx = await browser.new_context(user_agent=_UA)
            page = await ctx.new_page()
            page.on(
                "request",
                lambda r: captured.append(r.url) if _MEDIA_RE.search(r.url) else None,
            )
            try:
                await page.goto(page_url, wait_until="domcontentloaded", timeout=timeout * 1000)
            except Exception as exc:  # navigation timeout/abort — we may still have caught media
                logger.info("Headless-browser navigation issue (continuing)", error=str(exc))
            await page.wait_for_timeout(6000)
            for sel in _PLAY_SELECTORS:
                if any(_score(u) > 0 for u in captured):
                    break  # already have a candidate; no need to poke the player
                try:
                    el = await page.query_selector(sel)
                    if el:
                        await el.click(timeout=2000)
                        await page.wait_for_timeout(4000)
                except Exception:
                    pass
            await page.wait_for_timeout(3000)
            await browser.close()
    except Exception as exc:
        logger.warning("Headless-browser fallback failed to run", error=str(exc))
        return None

    candidates = [u for u in dict.fromkeys(captured) if _score(u) > 0]
    if not candidates:
        return None
    best = max(candidates, key=_score)
    return best, page_url
