{ makeTest, pkgs, module }:
let 
  litmus = pkgs.callPackage ./litmus.nix {};
in makeTest ({
  name = "check";
  nodes.machine1 = { ... }: {
    imports = [ module ];
    environment.systemPackages = [ litmus ];
    virtualisation = {
      diskSize = 2048;
      memorySize = 1024;
    };

    services.webdav_ss = {
      enable = true;
      host = "0.0.0.0";
      port = 5000;
      logLevel = "info";
      compression = false;
      prop_storage = {
        type = "yaml";
        path = "/tmp/twodir/yaml_storage.yml";
      };

      encryption = {
        enable = true;
        nonce = [ 1 2 3 4 5 6 7 8 9 10 11 12 ];
        phrase = [ 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25 26 27 28 29 30 31 32 ];
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
          auth = {
            type = "values";
            access_key_value = "minioadmin";
            secret_key_value = "minioadmin";
          };
        }
      ];

      environmentFile = pkgs.writeText "envs" "";
    };

    services.minio =
      let
        envFile = pkgs.writeText "envs" ''
          						MINIO_ROOT_USER=minioadmin
          						MINIO_ROOT_PASSWORD=minioadmin
          						MINIO_DOMAIN=localhost
          					'';
      in
      {
        enable = true;
        rootCredentialsFile = envFile;
      };
  };

  # skipLint = true;

  testScript = ''
    start_all()
    machine1.wait_for_unit("minio")
    machine1.wait_for_open_port(9000)
    machine1.wait_for_unit("webdav_ss.service")
    machine1.wait_for_open_port(5000)
    machine1.wait_until_succeeds("sleep 5")
    machine1.succeed("litmus http://localhost:5000/fs1")
    machine1.succeed("litmus http://localhost:5000/fs2")
    machine1.succeed("litmus http://localhost:5000/fs3")
  '';
}) { inherit pkgs; inherit (pkgs) system; }
