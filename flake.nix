{
  description = "rust workspace";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    let
      rust-version = "1.61.0";
      systems = [ "x86_64-linux" "aarch64-linux" ];
      base = system:
      let 
          pkgs = import nixpkgs { inherit system; overlays = [ rust-overlay.overlays.default ]; };
          lib = pkgs.lib;
          pkg = import ./Cargo.nix { inherit pkgs; };
          litmus = pkgs.stdenv.mkDerivation rec {
            pname = "litmus";
            version = "0.13";
            src = pkgs.fetchurl {
              url = "http://webdav.org/neon/litmus/litmus-${version}.tar.gz";
              sha256 = "sha256-CdYVlYEhcGRE22fgnEDfX3U8zx+hSEb960OSmKqaw/8=";
            };
          };
      in { inherit pkgs lib pkg litmus; };
      arch_flake = system:
        let
          spec_base = base system;
          pkgs = spec_base.pkgs;
          pkg = spec_base.pkg;
          litmus = spec_base.litmus;
          moduleTests = (import ./tests.nix { inherit system pkgs litmus; });
        in
        {
          defaultPackage = pkg.rootCrate.build;
          packages = {
            tests = moduleTests.driverInteractive;
          };

          checks.nixosTests = moduleTests.test;
        };

      shared_flake =
        let
          spec_base = base "x86_64-linux";
          pkgs = spec_base.pkgs;
          litmus = spec_base.litmus;
        in
        {
          nixosModules.webdav_ss = import ./module.nix;

          devShells.x86_64-linux.default = pkgs.mkShell
            {
              buildInputs = with pkgs; [
                rust-bin.stable.${rust-version}.default
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
                #export RUSTC_WRAPPER="${pkgs.sccache}/bin/sccache"
                export PATH=$PATH:$HOME/.cargo/bin
                export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"
              '';
            };
        };
    in
    (flake-utils.lib.eachSystem systems arch_flake) // shared_flake;
}
