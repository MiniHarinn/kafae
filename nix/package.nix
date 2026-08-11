{
  lib,
  stdenv,
  rustPlatform,
  installShellFiles,
}:
rustPlatform.buildRustPackage {
  pname = "kafae";
  version = "0.2.0";

  src = lib.fileset.toSource {
    root = ../.;
    fileset = lib.fileset.unions [
      ../Cargo.toml
      ../Cargo.lock
      ../build.rs
      ../src
      ../templates
    ];
  };

  cargoLock.lockFile = ../Cargo.lock;

  doCheck = stdenv.hostPlatform.config == stdenv.buildPlatform.config;

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
