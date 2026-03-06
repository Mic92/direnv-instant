"""Test that direnv exports variables even when it takes longer than TMUX_DELAY."""

from __future__ import annotations

import time
from pathlib import Path
from typing import TYPE_CHECKING

from tests.helpers import (
    allow_direnv,
    setup_envrc,
    setup_stub_tmux,
    setup_test_env,
)

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch

    from tests.conftest import DirenvInstantRunner, SignalWaiter


def test_slow_direnv_exports_via_tmux(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    direnv_instant: DirenvInstantRunner,
    signal_waiter: SignalWaiter,
) -> None:
    """Test direnv exports vars when it takes longer than TMUX_DELAY."""
    done_marker = tmp_path / "envrc_done"
    setup_envrc(
        tmp_path,
        f"""echo "Starting build..." >&2
while [ ! -f {done_marker} ]; do sleep 0.1; done
echo "Build complete!" >&2
export FOO=bar
export BAZ=qux
""",
    )

    tmux_called_file = tmp_path / "tmux_called"
    watch_output_file = tmp_path / "watch_output"

    setup_stub_tmux(
        tmp_path,
        f"""touch {tmux_called_file}
log_path="${{@: -2:1}}"
socket_path="${{@: -1}}"
{direnv_instant.binary_path} watch "$log_path" "$socket_path" \\
  > {watch_output_file} 2>&1 &""",
    )

    allow_direnv(tmp_path, monkeypatch)

    env = setup_test_env(tmp_path, signal_waiter.pid)

    # Run direnv-instant start (should not block)
    start_time = time.time()
    result = direnv_instant.run(["start"], env)
    elapsed = time.time() - start_time

    # Should complete quickly (not wait for direnv to finish)
    assert elapsed < 2, f"direnv-instant blocked for {elapsed}s"
    assert result.returncode == 0, f"Failed: {result.stderr}"

    # Parse env file path from output
    env_file_path = None
    for line in result.stdout.splitlines():
        if "__DIRENV_INSTANT_ENV_FILE" in line:
            # Extract path from: export __DIRENV_INSTANT_ENV_FILE='/path/to/file'
            env_file_path = line.split("=", 1)[1].strip().strip("'\"")
            break
    assert env_file_path, "Could not find __DIRENV_INSTANT_ENV_FILE in output"

    # Wait for tmux to be called (after 1s delay)
    timeout = 5
    start = time.time()
    while not tmux_called_file.exists() and (time.time() - start) < timeout:
        time.sleep(0.1)
    assert tmux_called_file.exists(), f"tmux stub was not called after {timeout}s"

    # Unblock .envrc by creating marker file
    done_marker.touch()

    # Wait for daemon to complete by polling for the env file
    env_file = Path(env_file_path)
    assert signal_waiter.wait_for_env_file(env_file, timeout=30), (
        "Env file not created within timeout"
    )

    # Verify environment variables were exported
    env_content = env_file.read_text()
    assert "FOO" in env_content, "FOO not found in env file"
    assert "bar" in env_content, "bar not found in env file"
    assert "BAZ" in env_content, "BAZ not found in env file"
    assert "qux" in env_content, "qux not found in env file"

    # Wait for watch output to be captured
    timeout_watch = 5
    start = time.time()
    while (time.time() - start) < timeout_watch:
        if watch_output_file.exists() and watch_output_file.stat().st_size > 0:
            break
        time.sleep(0.1)

    # Verify watch command was invoked and captured output
    assert watch_output_file.exists(), "watch output file was not created"
    watch_output = watch_output_file.read_text()
    assert len(watch_output) > 0, "watch command produced no output"
    # Verify we captured direnv output
    assert "Starting build" in watch_output or "direnv" in watch_output.lower()
