{
  lib,
  pkgs,
  kafae,
}:
let
  # everything the formatters look at
  fmtSource = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../src
      ../nix
      ../flake.nix
    ];
  };
in
{
  # builds the package and runs `cargo test`
  inherit kafae;

  clippy = kafae.overrideAttrs (old: {
    pname = "${old.pname}-clippy";
    nativeBuildInputs = old.nativeBuildInputs ++ [ pkgs.clippy ];
    buildPhase = "cargo clippy --all-targets -- -D warnings";
    installPhase = "touch $out";
    postInstall = "";
    doCheck = false;
  });

  fmt =
    pkgs.runCommand "kafae-fmt"
      {
        nativeBuildInputs = [
          pkgs.rustfmt
          pkgs.nixfmt
        ];
      }
      ''
        cd ${fmtSource}
        find . -name '*.rs' -exec rustfmt --edition 2021 --check {} +
        find . -name '*.nix' -exec nixfmt --check {} +
        touch $out
      '';
}
