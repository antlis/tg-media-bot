FROM python:3.12-slim

RUN apt-get update && \
    apt-get install -y --no-install-recommends ffmpeg && \
    pip install --no-cache-dir "yt-dlp[default,curl-cffi]" && \
    apt-get clean && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Chromium + its system libs for the headless-browser fallback (Playwright).
# Opt-in — it adds ~450 MB — via `--build-arg INSTALL_BROWSER=true` (or the
# INSTALL_BROWSER var in .env when building through docker compose). When off,
# the fallback simply no-ops and the bot reports the original failure.
ARG INSTALL_BROWSER=false
RUN if [ "$INSTALL_BROWSER" = "true" ]; then \
        playwright install --with-deps chromium && rm -rf /var/lib/apt/lists/*; \
    fi

COPY . .

# Entrypoint refreshes yt-dlp on start (toggle with YTDLP_AUTO_UPDATE) then
# launches the bot.
RUN chmod +x docker-entrypoint.sh
ENTRYPOINT ["./docker-entrypoint.sh"]
