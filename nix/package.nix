{
  lib,
  rustPlatform,
  installShellFiles,
}:
rustPlatform.buildRustPackage {
  pname = "kafae";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../src
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  nativeBuildInputs = [ installShellFiles ];

  postInstall = ''
    installShellCompletion --cmd kafae \
      --bash <(COMPLETE=bash $out/bin/kafae) \
      --zsh  <(COMPLETE=zsh  $out/bin/kafae) \
      --fish <(COMPLETE=fish $out/bin/kafae)
  '';
}
