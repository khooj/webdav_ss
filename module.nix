{ config, pkgs, lib, ... }:
let
	cfg = config.services.webdav_ss;
	webdav_ss = (import ./Cargo.nix { inherit pkgs; }).rootCrate.build;
	cfgFile = pkgs.writeText "config.yml" (builtins.toJSON {
		app = {
			inherit (cfg) host port;
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
	};

	config = mkIf cfg.enable {
		systemd.services.webdav_ss = {
			wantedBy = [ "multi-user.target" ];
			after = [ "network-online.target" ];
			script = "${cfg.package}/bin/webdav_ss -c ${cfgFile}";
			restartIfChanged = true;
			environment = {
				"RUST_LOG" = cfg.logLevel;
			} // cfg.environment;
			serviceConfig = {
				Restart = "on-failure";
				RestartSec = 5;
				TimeoutStartSec = 5;
			};
		};
	};
}