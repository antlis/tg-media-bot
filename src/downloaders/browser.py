"""Headless-browser fallback for pages whose player builds the media URL in
JavaScript.

yt-dlp only parses the static HTML a page ships. Some sites expose no stream
there — the player fetches (or assembles) the media URL at runtime via JS, often
only after the viewer clicks play or picks a source — so every extractor,
generic included, gives up with "Unsupported URL". This module loads such a page
in headless Chromium, drives the player (clicking play/source controls across
the page and any nested iframes), watches network traffic for the media request
the player makes, and returns that URL for yt-dlp to download normally.

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

# Things to click to make a player request its stream. Many players fetch the
# media only on play; streaming frontends inject the real player iframe only
# after a "source"/"server" tab is activated. These are common cross-site
# patterns, not any one site's markup.
_CLICK_SELECTORS = (
    ".vjs-big-play-button", ".jw-icon-display", ".jwplayer",
    ".plyr__control--overlaid", "[aria-label='Play']", "button[title='Play']",
    ".play-button", ".btn-play", ".play", "#player", "video",
    ".server-item", ".server", "[data-server-id]", ".servers .item",
    ".item.server", ".source-item",
)

# How many click-wait-recheck rounds to run before giving up.
_ROUNDS = 5

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


async def _poke(page) -> None:
    """Click play/source controls across the page and every nested frame.

    Cross-origin embeds run in their own frame; clicking a control inside one is
    how the actual stream request gets triggered. Everything is best-effort — a
    control that isn't there, isn't visible, or refuses the click is skipped.
    """
    for frame in page.frames:
        for sel in _CLICK_SELECTORS:
            try:
                els = await frame.query_selector_all(sel)
            except Exception:
                continue
            for el in els[:3]:
                try:
                    await el.click(timeout=1500, force=True)
                    await page.wait_for_timeout(400)
                except Exception:
                    pass


async def resolve_media_url(
    page_url: str, timeout: int = 45
) -> Optional[Tuple[str, str]]:
    """Return ``(media_url, referer)`` sniffed from the page's player, or None.

    ``referer`` is the exact Referer the browser sent for the captured request
    (the embed's origin, when the stream lives in an iframe) — most CDNs reject
    the stream without it. Falls back to the page URL when no Referer was sent.
    """
    try:
        from playwright.async_api import async_playwright
    except ImportError:
        logger.warning("Playwright not installed — skipping headless-browser fallback")
        return None

    captured: dict[str, str] = {}  # media url -> referer

    def _on_request(r) -> None:
        if _MEDIA_RE.search(r.url) and r.url not in captured:
            captured[r.url] = r.headers.get("referer") or page_url

    def _have_candidate() -> bool:
        return any(_score(u) > 0 for u in captured)

    try:
        async with async_playwright() as p:
            browser = await p.chromium.launch(
                headless=True,
                args=["--no-sandbox", "--disable-blink-features=AutomationControlled"],
            )
            ctx = await browser.new_context(
                user_agent=_UA,
                viewport={"width": 1280, "height": 720},
                locale="en-US",
            )
            # Hide the most obvious headless tell before any page script runs.
            await ctx.add_init_script(
                "Object.defineProperty(navigator,'webdriver',{get:()=>undefined});"
            )
            page = await ctx.new_page()
            page.on("request", _on_request)
            try:
                await page.goto(page_url, wait_until="domcontentloaded", timeout=timeout * 1000)
            except Exception as exc:  # navigation timeout/abort — we may still catch media
                logger.info("Headless-browser navigation issue (continuing)", error=str(exc))
            await page.wait_for_timeout(4000)  # let the initial player scripts settle
            for _ in range(_ROUNDS):
                if _have_candidate():
                    break
                await _poke(page)
                await page.wait_for_timeout(3000)  # give the stream request time to fire
            await browser.close()
    except Exception as exc:
        logger.warning("Headless-browser fallback failed to run", error=str(exc))
        return None

    candidates = [u for u in captured if _score(u) > 0]
    if not candidates:
        return None
    best = max(candidates, key=_score)
    return best, captured[best]
