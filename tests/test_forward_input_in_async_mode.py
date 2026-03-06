"""Test that arbitrary input is forwarded to direnv in async mode.

When direnv is running asynchronously in a multiplexer (tmux/zellij),
arbitrary input from the watch pane should be forwarded to the direnv
process running in the PTY, not just Ctrl-C.
"""

from __future__ import annotations

import subprocess
import time
from pathlib import Path
from typing import TYPE_CHECKING

from tests.helpers import (
    allow_direnv,
    setup_envrc,
    setup_test_env,
)

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch

    from tests.conftest import DirenvInstantRunner, SignalWaiter


def test_input_forwarded_to_direnv_in_async_mode(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    tmux_server: Path,
    direnv_instant: DirenvInstantRunner,
    signal_waiter: SignalWaiter,
) -> None:
    """Test that input from watch pane is forwarded to direnv process.

    This test verifies that when direnv is running asynchronously and
    prompts for input, typing in the watch pane forwards that input to
    the direnv process.
    """
    # Create an .envrc that waits for user input
    setup_envrc(
        tmp_path,
        """#!/usr/bin/env bash
# Prompt for input and export the result
echo "Enter your name:" >&2
read -r name
export USER_NAME="$name"
echo "Hello, $name!" >&2
""",
    )

    allow_direnv(tmp_path, monkeypatch)

    env = setup_test_env(tmp_path, signal_waiter.pid)
    # Set TMUX env var to use our isolated server
    env["TMUX"] = f"{tmux_server},0,0"

    # Run direnv-instant start (should not block)
    start_time = time.time()
    result = direnv_instant.run(["start"], env)
    elapsed = time.time() - start_time

    # Should complete quickly (not block waiting for input)
    assert elapsed < 3, f"direnv-instant blocked for {elapsed}s"
    assert result.returncode == 0, f"Failed: {result.stderr}"

    # Check that it exported the env vars
    assert "__DIRENV_INSTANT_ENV_FILE" in result.stdout
    assert "__DIRENV_INSTANT_STDERR_FILE" in result.stdout

    # Extract env file path
    env_file = None
    for line in result.stdout.splitlines():
        if "__DIRENV_INSTANT_ENV_FILE=" in line:
            env_file_str = line.split("=", 1)[1].strip().strip("'\"")
            env_file = Path(env_file_str)
            break
    assert env_file, "Could not find env file path"

    # Poll for watch pane to be created (with timeout)
    watch_pane_id = None
    start = time.time()
    timeout = 5
    while time.time() - start < timeout:
        list_panes = subprocess.run(
            [
                "tmux",
                "-S",
                str(tmux_server),
                "list-panes",
                "-F",
                "#{pane_id} #{pane_current_command}",
            ],
            capture_output=True,
            text=True,
            check=True,
        )

        for line in list_panes.stdout.splitlines():
            if "direnv-instant" in line or "watch" in line:
                watch_pane_id = line.split()[0]
                break

        if watch_pane_id:
            break

        time.sleep(0.1)

    assert watch_pane_id, (
        f"Watch pane not found after {timeout}s. Panes: {list_panes.stdout}"
    )

    # Wait a moment for direnv to start and prompt for input
    time.sleep(0.5)

    # Send input to the watch pane (type "Alice" and press Enter)
    subprocess.run(
        [
            "tmux",
            "-S",
            str(tmux_server),
            "send-keys",
            "-t",
            watch_pane_id,
            "Alice",
            "Enter",
        ],
        check=True,
        capture_output=True,
    )

    # Wait for the daemon to complete by polling for the env file
    assert signal_waiter.wait_for_env_file(env_file, timeout=10), (
        f"Environment file not created within timeout: {env_file}"
    )
    env_content = env_file.read_text()

    # Check that USER_NAME was set to our input
    assert "USER_NAME" in env_content, (
        f"USER_NAME not found in env file. Content:\n{env_content}"
    )
    assert "Alice" in env_content, (
        f"Input value 'Alice' not found in env file. Content:\n{env_content}"
    )
