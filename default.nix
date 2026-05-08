{
  lib,
  rustPlatform,
  direnv,
  tmux,
  fish,
  nushell,
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
      ./tests
    ];
  };

  cargoLock.lockFile = ./Cargo.lock;

  nativeCheckInputs = [
    direnv
    tmux
    fish
    nushell
  ];

  # Integration tests spawn shells and tmux servers; isolate them.
  preCheck = ''
    export HOME=$(mktemp -d)
    export TMPDIR=/tmp
  '';

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
