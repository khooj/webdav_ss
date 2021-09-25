{ system, pkgs, litmus }:
import "${toString pkgs.path}/nixos/tests/make-test-python.nix" ({ ... }: 
{
	name = "check";
	machine = { ... }: {
		imports = [ ./module.nix ];
		environment.systemPackages = [ litmus ];

		services.webdav_ss = {
			enable = true;
			host = "0.0.0.0";
			port = 5000;
			logLevel = "debug";
			filesystems = [
				{
					mount_path = "/fs1";
					type = "Mem";
				}
				{
					mount_path = "/fs2";
					path = "/tmp/webdav_ss/fs2";
					type = "FS";
				}
			];
		};
	};

	# skipLint = true;

	testScript = ''
start_all()
machine.wait_for_unit("webdav_ss.service")
machine.succeed("litmus http://localhost:5000/fs1")
# FS backend fails on few tests in "locks" and "props" suites
machine.succeed("TESTS=\"basic copymove http\" litmus http://localhost:5000/fs2")
'';
}) { inherit system; }