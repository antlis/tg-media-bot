#!/usr/bin/env python3
"""Example extractor plugin for tg-media-bot.

Copy into PLUGIN_DIR (./plugins with docker compose), `chmod +x` it and adapt
it to the site yt-dlp can't handle.

Protocol:
  argv[1]            the page URL the user sent
  env TGMB_MODE      "resolve" (tried before yt-dlp) or "fallback" (after
                     every yt-dlp extractor failed)
  exit code != 0     or empty stdout: "not my URL", the bot moves on
  stdout             a media URL (optionally followed by a Referer line), or
                     JSON: {"url": "...", "referer": "...", "headers": {...}}

The returned URL goes through the normal yt-dlp download, conversion and
upload path. Keep the "not mine" check cheap: every plugin is started for
every link.
"""
import json
import re
import sys

URL = re.compile(r"^https?://(?:www\.)?example\.com/watch/(?P<id>\w+)")


def main() -> int:
    m = URL.match(sys.argv[1] if len(sys.argv) > 1 else "")
    if not m:
        return 1  # not ours

    video_id = m.group("id")
    # Find the real media URL here: call the site's API, scrape the page...
    #   import urllib.request
    #   data = json.load(urllib.request.urlopen(f"https://example.com/api/{video_id}"))
    #   media = data["hls"]
    media = None
    if not media:
        return 1

    print(json.dumps({"url": media, "referer": sys.argv[1], "headers": {"Origin": "https://example.com"}}))
    return 0


if __name__ == "__main__":
    sys.exit(main())
