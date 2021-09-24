{ config, ... }:
{
	imports = [ ./module.nix ];
	services.webdav_ss = {
		enable = true;
		host = "0.0.0.0";
		filesystems = [
			{
				path = "/fs1";
				type = "Mem";
			}
		];
	};

	users.users.khooj = {
		isNormalUser = true;
		password = "khooj";
		extraGroups = [ "wheel" "sudo" ];
	};
}