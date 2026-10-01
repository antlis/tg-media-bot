{
  lib,
  stdenv,
  python3,
  makeWrapper,
  yt-dlp,
  ffmpeg,
  playwright-driver,
  # Headless-Chromium fallback for JS-only players (opt-in: large closure).
  withBrowser ? false,
}:

let
  pythonEnv = python3.withPackages (
    ps:
    [
      ps.aiogram
      ps.aiohttp
      ps.python-dotenv
      ps.structlog
    ]
    ++ lib.optional withBrowser ps.playwright
  );
in
stdenv.mkDerivation {
  pname = "tg-media-bot";
  # Keep in sync with the release tag (and packaging/aur/PKGBUILD).
  version = "0.6.0";

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../main.py
      ../../src
    ];
  };

  nativeBuildInputs = [ makeWrapper ];
  dontBuild = true;

  installPhase = ''
    runHook preInstall
    mkdir -p $out/share/tg-media-bot
    cp -r main.py src $out/share/tg-media-bot/
    makeWrapper ${pythonEnv.interpreter} $out/bin/tg-media-bot \
      --add-flags $out/share/tg-media-bot/main.py \
      --prefix PATH : ${
        lib.makeBinPath [
          yt-dlp
          ffmpeg
        ]
      } ${
        lib.optionalString withBrowser "--set-default PLAYWRIGHT_BROWSERS_PATH ${playwright-driver.browsers}"
      }
    runHook postInstall
  '';

  meta = {
    description = "Self-hosted Telegram media downloader bot (yt-dlp + aiogram)";
    homepage = "https://github.com/antlis/tg-media-bot";
    license = lib.licenses.mit;
    mainProgram = "tg-media-bot";
    platforms = lib.platforms.linux;
  };
}
