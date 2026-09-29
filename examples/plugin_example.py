"""Example custom extractor plugin for tg-media-bot.

Copy this file into your plugin directory (the ``./plugins`` folder mounted into
the Docker container, or the directory named by the ``PLUGIN_DIR`` env var),
rename it, and edit it for the site you want to support. Do NOT leave this
example in the plugin dir — its match() never fires, but keep the dir tidy.

A plugin is a ``.py`` file exposing two module-level callables:

    match(url)   -> bool          : claim the URLs you handle
    resolve(url) -> media source  : turn the page URL into something yt-dlp can
                                    download, or None to fall through

resolve() may be sync or async and may return:
    * a media URL string,
    * a (media_url, referer) tuple,
    * a ResolveResult(media_url, referer=..., headers={...}), or
    * None.

The returned media URL is handed to yt-dlp exactly like a normal download, so
format selection, recoding and upload all still apply. A plugin that raises is
logged and skipped — it can never crash a download.
"""

import re

# Matches example.com/watch/<id>. Replace with the site you want to support.
_URL = re.compile(r"^https?://(?:www\.)?example\.com/watch/(?P<id>\w+)")


def match(url: str) -> bool:
    return bool(_URL.match(url))


async def resolve(url: str):
    m = _URL.match(url)
    if not m:
        return None
    video_id = m.group("id")

    # Do whatever it takes to find the real media URL: call the site's API,
    # scrape the page, decrypt an embed, etc. aiohttp is available in the bot
    # environment (and yt-dlp ships requests / curl_cffi if you import them).
    #
    #   import aiohttp
    #   async with aiohttp.ClientSession() as s:
    #       async with s.get(f"https://example.com/api/sources/{video_id}") as r:
    #           data = await r.json()
    #   # hand back the manifest + the page as Referer:
    #   return data["hls"], url
    #
    # Return None to fall through to yt-dlp and the headless-browser fallback.
    _ = video_id
    return None
