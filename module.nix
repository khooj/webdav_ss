webdav_ss:
{ config, pkgs, ... }:
let
  cfg = config.services.webdav_ss;
  # webdav_ss = (import ./Cargo.nix { inherit pkgs; }).rootCrate.build;
  cfgFile = pkgs.writeText "config.yml" (builtins.toJSON {
    app = {
      inherit (cfg) host port ;
    };
    inherit (cfg) prop_storage filesystems encryption compression;
  });
in
with pkgs.lib;
{
  options.services.webdav_ss = {
    enable = mkEnableOption "webdav_ss";

    package = mkOption {
      type = types.package;
      description = "webdav_ss package";
      default = webdav_ss;
    };

    host = mkOption {
      type = types.str;
      description = "Listen host";
      default = "127.0.0.1";
    };

    port = mkOption {
      type = types.int;
      description = "Listen port";
      default = 5656;
    };

    compression = mkOption {
      type = with types; nullOr bool;
      description = "enable compression";
      default = false;
    };

    prop_storage = mkOption {
      type = with types; attrs;
      description = "props storage backend";
      default = null;
    };

    encryption = mkOption {
      type = with types; attrs;
      description = "encryption options";
      default = null;
    };

    logLevel = mkOption {
      type = types.enum [ "error" "warn" "info" "debug" ];
      description = "Log level";
      default = "error";
    };

    filesystems = with types; let
      subtype = attrs;
    in
    mkOption {
      type = listOf subtype;
      description = "Mounted filesystems";
      default = [ ];
    };

    environment = mkOption {
      type = types.attrs;
      description = "Environment variables";
      default = { };
    };

    environmentFile = mkOption {
      type = with types; nullOr (either path str);
      description = "Environment file";
      default = null;
    };
  };

  config.systemd.services.webdav_ss = mkIf cfg.enable {
    wantedBy = [ "multi-user.target" ];
    script = "${cfg.package}/bin/webdav_ss -c ${cfgFile}";
    restartIfChanged = true;
    environment = {
      "RUST_LOG" = cfg.logLevel;
    } // cfg.environment;
    serviceConfig = {
      EnvironmentFile = mkIf (cfg.environmentFile != null) (toString cfg.environmentFile);
      Restart = "on-failure";
      RestartSec = 5;
      TimeoutSec = 10;
    };
  };
}
