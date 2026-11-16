# Home Manager module: run tg-media-bot as a systemd user service.
# Usage: services.tg-media-bot = { enable = true; environmentFile = "/path/env"; };
self:
{ lib, pkgs, config, ... }:
let
  cfg = config.services.tg-media-bot;
in
{
  options.services.tg-media-bot = {
    enable = lib.mkEnableOption "tg-media-bot";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      description = "tg-media-bot package to run.";
    };

    environmentFile = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        Environment file with at least BOT_TOKEN (see .env.example).
        Keep it owner-readable only; it holds your bot token.
      '';
    };

    settings = lib.mkOption {
      type = lib.types.attrsOf lib.types.str;
      default = { };
      description = "Extra environment variables (ALLOWED_USERS, PROXY_URL, …).";
    };
  };

  config = lib.mkIf cfg.enable {
    systemd.user.services.tg-media-bot = {
      Unit = {
        Description = "tg-media-bot — self-hosted Telegram media downloader";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };

      Service = {
        ExecStart = lib.getExe cfg.package;
        Environment = lib.mapAttrsToList (n: v: "${n}=${v}") cfg.settings;
        EnvironmentFile = lib.optional (cfg.environmentFile != null) cfg.environmentFile;
        StateDirectory = "tg-media-bot";
        WorkingDirectory = "%S/tg-media-bot";
        Restart = "on-failure";
        RestartSec = "10s";
      };

      Install = {
        WantedBy = [ "default.target" ];
      };
    };
  };
}