"""Test that the watch command propagates terminal size to the PTY.

When direnv runs inside a PTY, programs like nix query the terminal
width via ioctl(TIOCGWINSZ) and truncate output to fit. The watch
command should propagate the actual tmux pane size to the PTY master
so output isn't needlessly truncated (issue #49).
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


def test_watch_propagates_terminal_size(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    direnv_instant: DirenvInstantRunner,
    signal_waiter: SignalWaiter,
) -> None:
    """Test that the watch pane's terminal size is propagated to the PTY.

    The .envrc script reports the terminal width seen by the child
    process. We create a wide (200-column) tmux session so the pane
    width differs from the hardcoded 80-column PTY default. If the
    watch command propagates its pane size, the child should see the
    pane's actual width.
    """
    done_marker = tmp_path / "envrc_done"
    cols_file = tmp_path / "cols_seen"

    # .envrc that waits, then reports terminal columns visible on stderr
    setup_envrc(
        tmp_path,
        f"""#!/usr/bin/env bash
echo "waiting..." >&2
while [ ! -f {done_marker} ]; do sleep 0.1; done
# Report the terminal width the child process sees
cols=$(stty size </dev/stderr 2>/dev/null | awk '{{print $2}}')
echo "$cols" > {cols_file}
echo "cols=$cols" >&2
export DONE=1
""",
    )

    allow_direnv(tmp_path, monkeypatch)

    # Create a wide tmux session (200 columns) so the pane width
    # differs from the hardcoded 80-column PTY default.
    socket_path = tmp_path / "tmux-socket"
    subprocess.run(
        [
            "tmux",
            "-S",
            str(socket_path),
            "new-session",
            "-d",
            "-x",
            "200",
            "-y",
            "50",
        ],
        check=True,
        capture_output=True,
    )

    try:
        env = setup_test_env(tmp_path, signal_waiter.pid)
        env["TMUX"] = f"{socket_path},0,0"

        # Run direnv-instant start
        result = direnv_instant.run(["start"], env)
        assert result.returncode == 0, f"Failed: {result.stderr}"

        env_file = None
        for line in result.stdout.splitlines():
            if "__DIRENV_INSTANT_ENV_FILE=" in line:
                env_file_str = line.split("=", 1)[1].strip().strip("'\"")
                env_file = Path(env_file_str)
                break
        assert env_file, "Could not find env file path"

        # Wait for watch pane to appear
        watch_pane_id = None
        start = time.time()
        while time.time() - start < 10:
            list_panes = subprocess.run(
                [
                    "tmux",
                    "-S",
                    str(socket_path),
                    "list-panes",
                    "-a",
                    "-F",
                    "#{pane_id} #{pane_width} #{pane_current_command}",
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

        assert watch_pane_id, f"Watch pane not found. Panes: {list_panes.stdout}"

        # Get the watch pane's actual width
        pane_width_result = subprocess.run(
            [
                "tmux",
                "-S",
                str(socket_path),
                "display-message",
                "-t",
                watch_pane_id,
                "-p",
                "#{pane_width}",
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        pane_width = int(pane_width_result.stdout.strip())
        assert pane_width > 80, (
            f"Pane width is {pane_width}, expected >80 for this test"
        )

        # Unblock the .envrc so it reports the terminal width
        done_marker.touch()

        # Wait for completion
        assert signal_waiter.wait_for_env_file(env_file, timeout=30), (
            "Env file not created within timeout"
        )

        # Read the terminal width the child process saw
        assert cols_file.exists(), "cols_seen file was not created"
        cols_seen = int(cols_file.read_text().strip())

        # The child should see the pane's width, not the hardcoded 80
        assert cols_seen == pane_width, (
            f"Child saw {cols_seen} columns but pane is {pane_width} wide. "
            f"Watch command did not propagate terminal size."
        )

    finally:
        subprocess.run(
            ["tmux", "-S", str(socket_path), "kill-server"],
            check=False,
            capture_output=True,
        )
