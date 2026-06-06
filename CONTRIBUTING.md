# Contributing

Thanks for your interest in improving tg-media-bot! This is a small, focused
utility bot — contributions that keep it lean and dependency-light are most
welcome.

## Getting set up

```bash
git clone https://github.com/antlis/tg-media-bot.git
cd tg-media-bot
python -m venv venv && source venv/bin/activate
pip install -r requirements-dev.txt
```

You'll also need `yt-dlp` and `ffmpeg` on your PATH for end-to-end runs (the
test suite mocks yt-dlp and self-skips the ffmpeg tests if it's missing).

## Before opening a PR

1. **Run the tests** — they're fast and need no network or Telegram access:
   ```bash
   pytest -q
   ```
   CI runs the same suite on Python 3.11 and 3.12.

2. **Keep config in one place.** All settings flow through
   `src/config/settings.py`. Add a field to `Settings`, read the env var in
   `_load_settings()`, and document it in `README.md` and `.env.example`. Don't
   call `os.getenv` elsewhere.

3. **No `shell=True`.** Subprocess calls (yt-dlp, ffmpeg) use argument arrays.

4. **Match the existing style** — structured logging via `get_logger()` rather
   than `print`, type hints on public functions, docstrings on modules/classes.

5. **Add a test** for new behaviour where it's practical (the pure-logic units
   are all covered today — config, URL extraction, command building, the queue,
   the caption builder, etc.).

## Project layout

See [ARCHITECTURE.md](ARCHITECTURE.md) for the request flow and module
responsibilities, and [CLAUDE.md](CLAUDE.md) for gotchas learned the hard way
(Docker rebuilds, the local Bot API server, healthchecks).

## Scope

This bot is intentionally a pure downloader — no AI/LLM calls, no account
system, no web UI. Features that broaden that scope are likely to be declined;
open an issue first if you're unsure whether something fits.
