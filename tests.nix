{
  lib,
  runCommand,
  python3,
  direnv-instant,
  direnv,
  tmux,
  fish,
}:

let
  testSrc = lib.fileset.toSource {
    root = ./.;
    fileset = ./tests;
  };

  pythonEnv = python3.withPackages (
    ps: with ps; [
      pytest
      pytest-xdist
    ]
  );
in
runCommand "direnv-instant-tests"
  {
    nativeBuildInputs = [
      pythonEnv
      direnv
      tmux
      fish
    ];

    meta = with lib; {
      description = "Integration tests for direnv-instant";
      license = licenses.mit;
    };
  }
  ''
    # Set TMPDIR to /tmp to avoid Unix socket path length limits (104 bytes on macOS)
    export TMPDIR=/tmp
    export HOME=$(mktemp -d)
    export DIRENV_INSTANT_BIN="${direnv-instant}/bin/direnv-instant"

    cp -r ${testSrc}/tests .
    chmod -R u+w tests

    # Run tests in parallel
    pytest tests/ -v -n auto

    mkdir -p $out
    echo "Tests passed" > $out/test-results
  ''
