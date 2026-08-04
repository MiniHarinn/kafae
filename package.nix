{
  lib,
  rustPlatform,
  installShellFiles,
  makeWrapper,
  tdf,
}:
rustPlatform.buildRustPackage {
  pname = "kafae";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  nativeBuildInputs = [
    installShellFiles
    makeWrapper
  ];

  postInstall = ''
    wrapProgram $out/bin/kafae --prefix PATH : ${lib.makeBinPath [ tdf ]}
    installShellCompletion --cmd kafae \
      --bash <(COMPLETE=bash $out/bin/kafae) \
      --zsh  <(COMPLETE=zsh  $out/bin/kafae) \
      --fish <(COMPLETE=fish $out/bin/kafae)
  '';
}
