{ pkgs, config, ... }:
let
	cfg = config.services.webdav_ss;
	webdav_ss = (import ./Cargo.nix { inherit pkgs; }).rootCrate.build;
	# cfgFile = pkgs.writeText "config.yml" (builtins.toYAML {
		# app = {
		# 	inherit (cfg) host port;
		# };

		# filesystems = map cfg.filesystems (x: {
		# 	inherit (x) path type;
		# });
	# });
in
with pkgs.lib;
{
	options.services.webdav_ss = {
		enable = mkEnableOption "webdav_ss";

		# package = mkOption {
		# 	type = types.package;
		# 	description = "webdav_ss package";
		# 	default = webdav_ss;
		# };

		# host = mkOption {
		# 	type = types.str;
		# 	description = "Listen host";
		# 	default = "127.0.0.1";
		# };

		# port = mkOption {
		# 	type = types.int;
		# 	description = "Listen port";
		# 	default = 5656;
		# };

		# filesystems = with types; mkOption {
		# 	type = listOf (submodule {
		# 		options = {
		# 			path = mkOption {
		# 				type = str;
		# 				description = "Mounted backend path";
		# 			};

		# 			type = mkOption {
		# 				type = enum [ "FS" "Mem" ];
		# 				description = "Mounted backend type";
		# 			};
		# 		};
		# 	});
		# 	description = "Mounted filesystems";
		# };
	};

	config = mkIf cfg.enable {
		# systemd.services.webdav_ss = {
			# wants = [ "multi-user.target" "network-online.target" ];
			# script = "${cfg.package}/bin/webdav_ss";
			# serviceType = {
			# 	Restart = "on-failure";
			# 	TimeoutSec = 3;
			# };
		# };
	};
}