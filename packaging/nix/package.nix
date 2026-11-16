{
  lib,
  rustPlatform,
  makeWrapper,
  yt-dlp,
  ffmpeg,
  deno,
}:

rustPlatform.buildRustPackage {
  pname = "tg-media-bot";
  # Keep in sync with Cargo.toml (and release tags).
  version = "1.0.6";

  src = lib.fileset.toSource {
    root = ../..;
    fileset = lib.fileset.unions [
      ../../Cargo.toml
      ../../Cargo.lock
      ../../src
    ];
  };

  cargoLock.lockFile = ../../Cargo.lock;

  nativeBuildInputs = [ makeWrapper ];

  # yt-dlp is the download engine; ffmpeg/ffprobe transcode and probe; deno
  # executes YouTube's JS challenges (see README "Run without Docker").
  postInstall = ''
    wrapProgram $out/bin/tg-media-bot \
      --prefix PATH : ${lib.makeBinPath [ yt-dlp ffmpeg deno ]}
  '';

  # No Rust tests are wired here; CI runs `cargo test --locked` in-tree.
  doCheck = false;

  meta = {
    description = "Lightweight self-hosted Telegram media downloader bot (yt-dlp + ffmpeg)";
    license = lib.licenses.mit;
    mainProgram = "tg-media-bot";
  };
}