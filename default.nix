{
  lib,
  rustPlatform,
}:

rustPlatform.buildRustPackage {
  pname = "direnv-instant";
  version = "1.1.0";

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

  # Nushell's `source` is a parse-time keyword and cannot read command
  # output, so ship the hook as a file users can source by path.
  postInstall = ''
    install -Dm644 hooks/nushell.nu $out/share/direnv-instant/nushell.nu
  '';

  meta = with lib; {
    description = "Non-blocking direnv integration daemon with tmux support";
    license = licenses.mit;
    mainProgram = "direnv-instant";
  };
}
