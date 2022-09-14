{
  description = "rust workspace";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay, ... }:
    let
      rust-version = "1.61.0";
      system = "x86_64-linux";
      pkgs = import nixpkgs {
        inherit system;
        overlays = [
          rust-overlay.overlays.default
          (self: super: {
            inherit (super.rust-bin.stable.${rust-version}) cargo rustc rustfmt rust-std clippy;
            # rustc = super.rust-bin.stable.${rust-version}.default;
          })
        ];
      };

      moduleTests = import ./tests.nix {
        makeTest = import "${pkgs.path}/nixos/tests/make-test-python.nix";
        inherit pkgs;
      };
    in
    {
      packages.${system} = {
        # tests = moduleTests;
        webdav_ss = (import ./Cargo.nix { inherit pkgs; }).rootCrate.build;
      };

      checks.${system}.nixosTests = moduleTests.test;

      nixosModules.webdav_ss = import ./module.nix;

      devShells.x86_64-linux.default = pkgs.mkShell
        {
          buildInputs = with pkgs; with rust-bin.stable.${rust-version}; [
            rustc
            cargo
            rustfmt
            clippy
            rust-std

            litmus
            pkgconfig
            gnumake
            jq
            git
            bintools
            python3
            openssl
            cmake
            crate2nix
            nixos-shell
            vscodium
          ];
          nativeBuildInputs = with pkgs; [ rustc cargo pkgconfig nixpkgs-fmt ];
          shellHook = ''
            export PATH=$PATH:$HOME/.cargo/bin
            export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"
          '';
        };
    };
}
