{
  description = "kafae: terminal client for Cafe Grader";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { self, nixpkgs }:
    let
      inherit (nixpkgs) lib;

      systems = [
        "x86_64-linux"
        "aarch64-linux"
        "aarch64-darwin"
      ];
      forAllSystems = f: lib.genAttrs systems (system: f nixpkgs.legacyPackages.${system});
    in
    {
      packages = forAllSystems (pkgs: rec {
        kafae = pkgs.callPackage ./nix/package.nix { };
        kafae-windows = pkgs.pkgsCross.mingwW64.callPackage ./nix/package.nix { };
        # release binary for machines without a nix store
        kafae-static = pkgs.pkgsStatic.callPackage ./nix/package.nix { };
        default = kafae;
      });

      checks = forAllSystems (
        pkgs:
        import ./nix/checks.nix {
          inherit lib pkgs;
          kafae = self.packages.${pkgs.stdenv.hostPlatform.system}.kafae;
        }
        # keep cfg(windows) code compiling; the cross toolchain is only cached for x86_64-linux
        // lib.optionalAttrs (pkgs.stdenv.hostPlatform.system == "x86_64-linux") {
          windows = self.packages.x86_64-linux.kafae-windows;
          # the release binary, so a break shows up here and not at tag time
          static = self.packages.x86_64-linux.kafae-static;
        }
      );

      formatter = forAllSystems (pkgs: pkgs.nixfmt-tree);

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          inputsFrom = [ self.packages.${pkgs.stdenv.hostPlatform.system}.default ];
          packages = [
            pkgs.clippy
            pkgs.rustfmt
            pkgs.nixfmt
          ];
        };
      });
    };
}
