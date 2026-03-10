"""Test that an empty direnv export does not overwrite a valid cached env file.

Regression test: when direnv export returns success but produces no output
(environment already up-to-date), the daemon must not overwrite the previously
cached env file with an empty one.
"""

from __future__ import annotations

import json
import subprocess
import time
from pathlib import Path
from typing import TYPE_CHECKING

from tests.conftest import SignalWaiter
from tests.helpers import (
    allow_direnv,
    setup_envrc,
    setup_stub_tmux,
    setup_test_env,
)

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch

    from tests.conftest import DirenvInstantRunner


def test_empty_export_preserves_cache(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    direnv_instant: DirenvInstantRunner,
    signal_waiter: SignalWaiter,
) -> None:
    """A second direnv run with empty output must not clobber the cached env file."""
    setup_envrc(tmp_path, "export MY_TEST_VAR=hello\n")
    setup_stub_tmux(tmp_path)
    allow_direnv(tmp_path, monkeypatch)

    env = setup_test_env(tmp_path, signal_waiter.pid)

    # First run: populates the env cache file
    result = direnv_instant.run(["start"], env)
    assert result.returncode == 0, f"First start failed: {result.stderr}"

    env_file_path = None
    for line in result.stdout.splitlines():
        if "__DIRENV_INSTANT_ENV_FILE" in line:
            env_file_path = line.split("=", 1)[1].strip().strip("'\"")
            break
    assert env_file_path, "Could not find __DIRENV_INSTANT_ENV_FILE in output"
    env_file = Path(env_file_path)

    assert signal_waiter.wait_for_env_file(env_file, timeout=30), (
        "Env file not created after first run"
    )
    assert "MY_TEST_VAR" in env_file.read_text()

    # Simulate the shell having eval'd the env file by asking direnv itself
    # what variables it would export, then injecting them all.
    direnv_out = subprocess.run(
        ["direnv", "export", "json"],
        capture_output=True,
        text=True,
        check=True,
    )
    for key, val in json.loads(direnv_out.stdout).items():
        env[key] = val

    # Second run: direnv sees env is already loaded, exports nothing
    waiter2 = SignalWaiter()
    try:
        env["DIRENV_INSTANT_SHELL_PID"] = str(waiter2.pid)
        result2 = direnv_instant.run(["start"], env)
        assert result2.returncode == 0, f"Second start failed: {result2.stderr}"

        # Daemon produces no output so no SIGUSR1 is sent; just wait for it
        # to finish (socket disappears).
        socket_path = env_file.parent / "daemon.sock"
        start = time.time()
        while socket_path.exists() and (time.time() - start) < 10:
            time.sleep(0.2)

        assert "MY_TEST_VAR" in env_file.read_text(), (
            "Env file was clobbered by empty export"
        )
    finally:
        waiter2.cleanup()
