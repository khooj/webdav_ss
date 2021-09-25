{ config, pkgs, lib, ... }:
let
	cfg = config.services.webdav_ss;
	webdav_ss = (import ./Cargo.nix { inherit pkgs; }).rootCrate.build;
	cfgFile = pkgs.writeText "config.yml" (builtins.toJSON {
		app = {
			inherit (cfg) host port;
		};

		filesystems = map (x: {
			inherit (x) type mount_path;
			path = if (x.path != null) then x.path else null;
		}) cfg.filesystems;
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
			subtype = submodule {
				options = {
					path = mkOption {
						type = nullOr str;
						description = "(FS Only) system path";
						default = null;
					};

					mount_path = mkOption {
						type = str;
						description = "Mount path";
						default = "/";
					};

					type = mkOption {
						type = enum [ "FS" "Mem" ];
						description = "Mounted backend type";
					};
				};
			};
		in mkOption {
			type = listOf subtype;
			description = "Mounted filesystems";
			default = [];
		};
	};

	config = mkIf cfg.enable {
		systemd.services.webdav_ss = {
			wantedBy = [ "multi-user.target" "network-online.target" ];
			script = "${cfg.package}/bin/webdav_ss -c ${cfgFile}";
			restartIfChanged = true;
			environment = {
				"RUST_LOG" = cfg.logLevel;
			};
			serviceConfig = {
				Restart = "on-failure";
				RestartSec = 3;
			};
		};
	};
}