{ system, pkgs, litmus }:
import "${toString pkgs.path}/nixos/tests/make-test-python.nix" ({ lib, ... }: 
{
	name = "check";
	machine = { ... }: {
		imports = [ ./module.nix ];
		environment.systemPackages = [ litmus ];

		services.webdav_ss = {
			enable = true;
			host = "0.0.0.0";
			port = 5000;
			logLevel = "info";
			environment = {
				"AWS_ACCESS_KEY_ID" = "minioadmin";
			};
			filesystems = [
				{
					mount_path = "/fs1";
					type = "mem";
				}
				{
					mount_path = "/fs2";
					path = "/tmp/webdav_ss/fs2";
					type = "fs";
				}
				{
					type = "s3";
					mount_path = "/fs3";
					url = "http://localhost:9000";
					bucket = "test";
					region = "us-east-1";
					path_style = false;
					ensure_bucket = true;
				}
			];

			environmentFile = pkgs.writeText "envs" ''
			AWS_SECRET_ACCESS_KEY=minioadmin
			'';
		};

		services.minio = let
			envFile = pkgs.writeText "envs" ''
			MINIO_ROOT_USER=minioadmin
			MINIO_ROOT_PASSWORD=minioadmin
			MINIO_DOMAIN=localhost
			'';
		in {
			enable = true;
			rootCredentialsFile = envFile;
		};
	};

	# skipLint = true;

	testScript = ''
start_all()
machine.wait_for_unit("minio")
machine.wait_for_open_port(9000)
machine.wait_for_unit("webdav_ss.service")
machine.wait_for_open_port(5000)
machine.succeed("litmus http://localhost:5000/fs1")
machine.succeed("litmus http://localhost:5000/fs2")
machine.succeed("litmus http://localhost:5000/fs3")
'';
}) { inherit system; }