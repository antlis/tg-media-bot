# syntax=docker/dockerfile:1

ARG DENO_VERSION=2.5.6
ARG FFMPEG_VERSION=7.1.1

FROM denoland/deno:bin-${DENO_VERSION} AS deno
# Static ffmpeg/ffprobe: ~200 MB lighter than Debian's package and newer.
FROM mwader/static-ffmpeg:${FFMPEG_VERSION} AS ffmpeg

# ---- build ------------------------------------------------------------------
FROM rust:1-slim-bookworm AS build
ARG TARGETARCH
WORKDIR /src
# Ride out flaky DNS/network in build sandboxes.
ENV CARGO_NET_RETRY=10

# Compile for modern x86-64-v3 (AVX2, BMI2) on amd64; use default native on arm64.
RUN if [ "$TARGETARCH" = "amd64" ] || [ -z "$TARGETARCH" ]; then \
        mkdir -p /root/.cargo && printf '[build]\nrustflags = ["-C", "target-cpu=x86-64-v3"]\n' > /root/.cargo/config.toml; \
    fi
# Build dependencies first so source edits don't rebuild them.
COPY Cargo.toml Cargo.lock ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY src ./src
RUN touch src/main.rs && cargo build --release --locked

# ---- runtime ----------------------------------------------------------------
FROM python:3.12-slim-bookworm

RUN useradd --uid 1000 --create-home --shell /usr/sbin/nologin app \
    && python -m venv /opt/yt-dlp \
    && /opt/yt-dlp/bin/pip install --no-cache-dir "yt-dlp[default]" \
    && /opt/yt-dlp/bin/python -m compileall -q /opt/yt-dlp \
    && chown -R app:app /opt/yt-dlp \
    && mkdir -p /data/tmp /cookies /plugins \
    && chown -R app:app /data /cookies /plugins

COPY --from=deno /deno /usr/local/bin/deno
COPY --from=ffmpeg /ffmpeg /ffprobe /usr/local/bin/
COPY --from=build /src/target/release/tg-media-bot /usr/local/bin/tg-media-bot

ENV PATH=/opt/yt-dlp/bin:$PATH \
    DATA_DIR=/data \
    TEMP_DIR=/data/tmp \
    YTDLP_UPDATE_CMD="/opt/yt-dlp/bin/pip install --no-cache-dir -q -U yt-dlp[default] && /opt/yt-dlp/bin/python -m compileall -q /opt/yt-dlp" \
    MALLOC_ARENA_MAX=2 \
    DENO_NO_UPDATE_CHECK=1 \
    DENO_DIR=/data/tmp/deno_cache \
    DENO_V8_FLAGS="--jitless --max-old-space-size=64"

USER app
WORKDIR /data
VOLUME ["/data"]
ENTRYPOINT ["/usr/local/bin/tg-media-bot"]
