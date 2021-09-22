{ webdav_ss }:
{ config, pkgs, ... }:
let
	cfg = config.services.webdav_ss;
in
with pkgs.lib;
{
	options.webdav_ss = {
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

		filesystems = mkOption {
			type = types.arrayOf types.attr;
			description = "Mounted filesystems";
		};
	};

	config = mkIf cfg.enable {

	};
}