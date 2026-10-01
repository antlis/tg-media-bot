{
  description = "tg-media-bot — self-hosted Telegram media downloader (yt-dlp + aiogram)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAll (pkgs: rec {
        tg-media-bot = pkgs.callPackage ./packaging/nix/package.nix { };
        with-browser = tg-media-bot.override { withBrowser = true; };
        default = tg-media-bot;
      });

      overlays.default = final: _prev: {
        tg-media-bot = final.callPackage ./packaging/nix/package.nix { };
      };

      homeManagerModules.default = import ./packaging/nix/home-manager.nix self;

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            (pkgs.python3.withPackages (
              ps: with ps; [
                aiogram
                aiohttp
                python-dotenv
                structlog
                playwright
                pytest
                pytest-asyncio
              ]
            ))
            pkgs.yt-dlp
            pkgs.ffmpeg
          ];
        };
      });

      checks = forAll (pkgs: {
        package = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
        # The same suite CI runs with pip, against the Nix-provided deps.
        tests =
          pkgs.runCommand "tg-media-bot-tests"
            {
              nativeBuildInputs = [
                (pkgs.python3.withPackages (
                  ps: with ps; [
                    aiogram
                    aiohttp
                    python-dotenv
                    structlog
                    playwright
                    pytest
                    pytest-asyncio
                  ]
                ))
                pkgs.yt-dlp
                pkgs.ffmpeg
              ];
            }
            ''
              cp -r ${./.}/. src_tree && chmod -R u+w src_tree && cd src_tree
              HOME=$TMPDIR python -m pytest -p no:cacheprovider
              touch $out
            '';
      });
    };
}
