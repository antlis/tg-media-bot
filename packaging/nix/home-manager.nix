self:
{ config, lib, pkgs, ... }:

let
  cfg = config.services.tg-media-bot;
  inherit (lib) mkEnableOption mkOption mkIf types;
  state = "%S/tg-media-bot";
in
{
  options.services.tg-media-bot = {
    enable = mkEnableOption "tg-media-bot, a Telegram media downloader";

    package = mkOption {
      type = types.package;
      default = self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      defaultText = "tg-media-bot flake package";
    };

    environmentFile = mkOption {
      type = types.nullOr types.str;
      default = null;
      example = "/home/me/.config/tg-media-bot.env";
      description = ''
        File with BOT_TOKEN (and optionally ALLOWED_USERS, API_SERVER_URL, …),
        kept out of the Nix store. Pass it as a string, not a Nix path literal.
      '';
    };

    settings = mkOption {
      type = types.attrsOf types.str;
      default = { };
      example = {
        API_SERVER_URL = "http://localhost:8082";
        COOKIES_FILE = "/home/me/cookies/cookies.txt";
      };
      description = "Extra environment variables (see .env.example).";
    };
  };

  config = mkIf cfg.enable {
    systemd.user.services.tg-media-bot = {
      Unit = {
        Description = "tg-media-bot — Telegram media downloader";
        After = [ "network-online.target" ];
        Wants = [ "network-online.target" ];
      };
      Service = {
        ExecStart = lib.getExe cfg.package;
        Restart = "on-failure";
        RestartSec = 5;
        # ~/.local/state/tg-media-bot — allowlist, media cache, log, …
        StateDirectory = "tg-media-bot";
        Environment = lib.mapAttrsToList (k: v: "${k}=${v}") (
          {
            LOG_FILE = "${state}/tg-media-bot.log";
            ALLOWED_CHATS_FILE = "${state}/allowed_chats.json";
            MEDIA_CACHE_FILE = "${state}/media_cache.json";
            MINIMAL_MODE_FILE = "${state}/minimal_mode.json";
            TOPIC_LOCK_FILE = "${state}/topic_locks.json";
          }
          // cfg.settings
        );
        EnvironmentFile = mkIf (cfg.environmentFile != null) cfg.environmentFile;
      };
      Install.WantedBy = [ "default.target" ];
    };
  };
}
