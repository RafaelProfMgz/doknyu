{
  description = "doknyu: desktop manager for Docker containers";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      # The package is packaging/nix/package.nix (the file that goes to nixpkgs) built from this
      # tree: src and the vendored crates come from here instead of the release tag.
      packages = forAllSystems (pkgs: rec {
        doknyu = (pkgs.callPackage ./packaging/nix/package.nix { }).overrideAttrs (old: {
          src = pkgs.lib.cleanSource self;
          cargoDeps = pkgs.rustPlatform.importCargoLock { lockFile = ./Cargo.lock; };
        });
        default = doknyu;
      });

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.doknyu ];
          packages = with pkgs; [
            cargo
            clippy
            rustc
            rustfmt
          ];
          # dlopen'ed libraries are not linked, so `cargo run` needs them on the loader path.
          LD_LIBRARY_PATH = pkgs.lib.makeLibraryPath (
            with pkgs;
            [
              libGL
              libx11
              libxcb
              libxcursor
              libxi
              libxkbcommon
              vulkan-loader
              wayland
            ]
          );
        };
      });
    };
}
