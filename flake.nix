{
  description = "rust workspace";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, rust-overlay, flake-utils, ... }:
    let
      rust-version = "1.54.0";
      systems = [ "x86_64-linux" "aarch64-linux" ];
    in flake-utils.lib.eachSystem systems (system:
      let
        overlays = [
          rust-overlay.overlay
          (self: super: rec {
            rustc = self.rust-bin.stable.${rust-version}.default.override {
              extensions =
                [ "rust-src" "rust-std" "rustfmt" "clippy" "rust-analysis" ];
            };
            cargo = rustc;
          })
        ];
        pkgs = import nixpkgs { inherit system overlays; };
        lib = pkgs.lib;
        pkg = import ./Cargo.nix { inherit pkgs; };
        litmus = pkgs.stdenv.mkDerivation rec {
          pname = "litmus";
          version = "0.13";
          src = pkgs.fetchurl {
            url = "http://webdav.org/neon/litmus/litmus-${version}.tar.gz";
            sha256 = "sha256-CdYVlYEhcGRE22fgnEDfX3U8zx+hSEb960OSmKqaw/8=";
          };
          patches = [ ./litmus.patch ];
        };

        buildInputs = with pkgs; [
          litmus
          sccache
          pkgconfig
          gnumake
          jq
          git
          bintools
          llvmPackages.bintools
          llvmPackages.libcxxClang
          python3
          openssl
          cmake
          crate2nix
          nixos-shell
        ];
        nativeBuildInputs = with pkgs; [ rustc cargo pkgconfig nixpkgs-fmt ];
        moduleTests = (import ./tests.nix { inherit system pkgs litmus; });
      in rec {
        defaultPackage = pkg.rootCrate.build;
        packages = {
          tests = moduleTests.driverInteractive;
        };

        checks.nixosTests = moduleTests.test;

        devShell = with pkgs;
          mkShell {
            buildInputs = [ ] ++ buildInputs;
            inherit nativeBuildInputs;

            shellHook = ''
              #export RUSTC_WRAPPER="${pkgs.sccache}/bin/sccache"
              export PATH=$PATH:$HOME/.cargo/bin
              export PKG_CONFIG_PATH="${pkgs.openssl.dev}/lib/pkgconfig"
            '';
          };
      }) // {
        nixosModules.webdav_ss = import ./module.nix;
      };
}
