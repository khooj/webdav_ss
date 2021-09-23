{ pkgs, system }:
with import "${toString pkgs.path}/nixos/lib/testing-python.nix" { inherit system pkgs; };
with pkgs.lib;
makeTest {
	name = "check";
	machine = { ... }: {
		imports = [ ./module.nix ];
		services.webdav_ss = {
			enable = true;
			# host = "0.0.0.0";
			# port = 5000;
			# filesystems = [
			# 	{
			# 		path = "/fs1";
			# 		type = "Mem";
			# 	}
			# ];
		};
	};

	skipLint = true;

	testScript = ''
	start_all()

	machine.wait_for_unit("webdav_ss.service")
	'';
}