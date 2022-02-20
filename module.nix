{ config, pkgs, lib, ... }:
let
	cfg = config.services.webdav_ss;
	pkg = (import ./Cargo.nix { inherit pkgs; });
	webdav_ss = pkg.rootCrate.build;
	cfgFile = pkgs.writeText "config.yml" (builtins.toJSON {
		app = {
			inherit (cfg) host port prop_storage;
		};

		filesystems = cfg.filesystems;
	});
in
with lib;
{
	options.services.webdav_ss = {
		enable = mkOption {
			type = types.bool;
			default = false;
		};

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

		prop_storage = mkOption {
			type = with types; nullOr attrs;
			description = "props storage backend";
			default = null;
		};

		logLevel = mkOption {
			type = types.enum [ "error" "warn" "info" "debug" ];
			description = "Log level";
			default = "error";
		};

		filesystems = with types; let 
			subtype = attrs;
		in mkOption {
			type = listOf subtype;
			description = "Mounted filesystems";
			default = [];
		};

		environment = mkOption {
			type = types.attrs;
			description = "Environment variables";
			default = {};
		};

		environmentFile = mkOption {
			type = with types; nullOr (either path str);
			description = "Environment file";
			default = null;
		};
	};

	config = mkIf cfg.enable {
		systemd.services.webdav_ss = let
		in {
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
	};
}