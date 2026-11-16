{
  description = "tg-media-bot — lightweight self-hosted Telegram media downloader (Rust + yt-dlp)";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs = { self, nixpkgs }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAll = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in {
      packages = forAll (pkgs: rec {
        tg-media-bot = pkgs.callPackage ./packaging/nix/package.nix { };
        default = tg-media-bot;
      });

      overlays.default = final: _prev: {
        tg-media-bot = final.callPackage ./packaging/nix/package.nix { };
      };

      homeManagerModules.default = import ./packaging/nix/home-manager.nix self;

      devShells = forAll (pkgs: {
        default = pkgs.mkShell {
          packages = [
            pkgs.cargo
            pkgs.rustc
            pkgs.rustfmt
            pkgs.clippy
            pkgs.rust-analyzer
            # runtime deps, for local runs/tests
            pkgs.yt-dlp
            pkgs.ffmpeg
            pkgs.deno
          ];
        };
      });
    };
}