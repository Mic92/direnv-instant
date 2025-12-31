{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "direnv-instant";
  version = "0.1.0";

  src = lib.fileset.toSource {
    root = ./.;
    fileset = lib.fileset.unions [
      ./Cargo.toml
      ./Cargo.lock
      ./src
      ./hooks
    ];
  };

  cargoLock = {
    lockFile = ./Cargo.lock;
  };

  meta = with lib; {
    description = "Non-blocking direnv integration daemon with tmux support";
    license = licenses.mit;
    mainProgram = "direnv-instant";
  };
}
