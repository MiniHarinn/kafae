{
  lib,
  stdenv,
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
      ../build.rs
      ../src
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  # the default strip keeps the symbol table, a third of the mingw binary
  stripAllList = [ "bin" ];

  nativeBuildInputs = [ installShellFiles ];

  # completions come out of running the binary, so cross builds go without
  postInstall = lib.optionalString (stdenv.hostPlatform.canExecute stdenv.buildPlatform) ''
    installShellCompletion --cmd kafae \
      --bash <(COMPLETE=bash $out/bin/kafae) \
      --zsh  <(COMPLETE=zsh  $out/bin/kafae) \
      --fish <(COMPLETE=fish $out/bin/kafae)
  '';
}
