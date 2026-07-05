# h/t https://github.com/niri-wm/niri/blob/main/flake.nix
{
  description = "mcdl: Minecraft server management tool";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-parts.url = "github:hercules-ci/flake-parts";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-parts,
      ...
    }@inputs:
    let
      revision = self.shortRev or self.dirtyShortRev or "unknown";
      mcdl-package =
        {
          pkg-config,
          rustPlatform,
        }:
        rustPlatform.buildRustPackage (finalAttrs: {
          pname = "mcdl";
          version = revision;

          src = ./.;
          cargoLock.lockFile = ./Cargo.lock;

          MCDL_GIT_SHA = revision;
          MCDL_BUILD_DEBUG = "0";

          nativeBuildInputs = [ pkg-config ];

          useNextest = true;
          checkFlags = [
            # filter out tests that require network access or other external dependencies
            "types::"
            "utils::macros::"
            # install_jre and utils::net::* require network. mcdl::bin_tests tries to instantiate a
            # reqwest client which fails to load CA certs.
            # TODO: vm test?
          ];
        });

      inherit (nixpkgs) lib;
    in
    flake-parts.lib.mkFlake { inherit inputs; } {
      systems = lib.intersectLists lib.systems.flakeExposed lib.platforms.linux;

      perSystem =
        { self', pkgs, ... }:
        {
          checks = {
            inherit (self'.packages) mcdl-debug;
          };

          packages =
            let
              mcdl = pkgs.callPackage mcdl-package { };
            in
            {
              inherit mcdl;

              mcdl-debug = mcdl.overrideAttrs (
                final: prev: {
                  pname = "${prev.pname}-debug";

                  cargoBuildType = "debug";
                  cargoCheckType = final.cargoBuildType;

                  MCDL_BUILD_DEBUG = "1";
                }
              );

              default = mcdl;
            };
        };
    };
}
