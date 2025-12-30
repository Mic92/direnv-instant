"""Test fish shell integration for direnv-instant."""

from __future__ import annotations

import os
import shutil
import subprocess
import time
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from tests.helpers import allow_direnv, setup_envrc, setup_stub_tmux, setup_test_env

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch

    from tests.conftest import DirenvInstantRunner


def test_fish_integration_runs_direnv_synchronously(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    direnv_instant: DirenvInstantRunner,
) -> None:
    """Test that fish integration properly loads environment without tmux."""
    if shutil.which("fish") is None:
        pytest.skip("fish shell not available")

    setup_envrc(
        tmp_path,
        """sleep 1
export FISH_TEST=success
""",
    )
    allow_direnv(tmp_path, monkeypatch)

    # Get the hook code
    env = os.environ.copy()
    hook_result = direnv_instant.run(["hook", "fish"], env)
    assert hook_result.returncode == 0, f"hook fish failed: {hook_result.stderr}"

    # Create a fish script that sources the hook and checks the result
    fish_script = f"""
{hook_result.stdout}

# After hook runs, check if FISH_TEST is set
if set -q FISH_TEST
    echo "FISH_TEST=$FISH_TEST"
else
    echo "FISH_TEST not set"
end
"""

    # Run in fish without tmux
    env = os.environ.copy()
    env.pop("TMUX", None)
    env["PATH"] = f"{Path(direnv_instant.binary_path).parent}:{env['PATH']}"

    start_time = time.time()
    result = subprocess.run(
        ["fish", "--no-config", "--command", fish_script],
        check=False,
        capture_output=True,
        text=True,
        env=env,
        cwd=tmp_path,
    )
    elapsed = time.time() - start_time

    # Should block for at least the sleep duration
    assert elapsed >= 0.9, f"fish returned too quickly: {elapsed}s"
    assert result.returncode == 0, f"fish failed: {result.stderr}"
    assert "FISH_TEST=success" in result.stdout, f"Output: {result.stdout}"


def test_fish_async_creates_env_file(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    direnv_instant: DirenvInstantRunner,
) -> None:
    """Test that fish async mode creates env file with correct content."""
    if shutil.which("fish") is None:
        pytest.skip("fish shell not available")

    done_marker = tmp_path / "envrc_done"
    setup_envrc(
        tmp_path,
        f"""while [ ! -f {done_marker} ]; do sleep 0.1; done
export ASYNC_TEST=async_success
""",
    )

    setup_stub_tmux(tmp_path)
    allow_direnv(tmp_path, monkeypatch)

    # Get the hook code
    env = os.environ.copy()
    hook_result = direnv_instant.run(["hook", "fish"], env)
    assert hook_result.returncode == 0, f"hook fish failed: {hook_result.stderr}"

    # Fish script that:
    # 1. Sources hook (returns immediately in async mode)
    # 2. Captures the env file path
    # 3. Unblocks the envrc
    # 4. Polls for env file to have content
    result_file = tmp_path / "result.txt"
    fish_script = f"""
{hook_result.stdout}

# Save env file path
echo $__DIRENV_INSTANT_ENV_FILE > {result_file}

# Unblock envrc
echo "unblocking" > {done_marker}

# Poll for env file to have content (daemon writes it after direnv completes)
set -l attempts 0
while test $attempts -lt 100
    if test -s "$__DIRENV_INSTANT_ENV_FILE"
        echo "ENV_FILE_READY" >> {result_file}
        cat "$__DIRENV_INSTANT_ENV_FILE" >> {result_file}
        exit 0
    end
    sleep 0.1
    set attempts (math $attempts + 1)
end
echo "ENV_FILE_TIMEOUT" >> {result_file}
"""

    # Run in fish WITH tmux stub (async mode)
    env = setup_test_env(tmp_path, os.getpid())
    env["PATH"] = f"{Path(direnv_instant.binary_path).parent}:{env['PATH']}"

    result = subprocess.run(
        ["fish", "--no-config", "--command", fish_script],
        check=False,
        capture_output=True,
        text=True,
        env=env,
        cwd=tmp_path,
        timeout=15,
    )

    assert result.returncode == 0, (
        f"fish failed: {result.stderr}\nstdout: {result.stdout}"
    )
    assert result_file.exists(), "Result file not created"
    result_content = result_file.read_text()
    assert "ENV_FILE_READY" in result_content, f"Env file not ready: {result_content}"
    # Verify fish-format export (set -x -g, not bash export)
    assert "ASYNC_TEST" in result_content, f"Missing ASYNC_TEST: {result_content}"
    assert "async_success" in result_content, f"Missing value: {result_content}"
    assert "export" not in result_content, (
        f"Got bash syntax instead of fish: {result_content}"
    )
