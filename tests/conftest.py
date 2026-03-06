"""Pytest configuration and fixtures for direnv-instant tests."""

from __future__ import annotations

import os
import subprocess
import tempfile
import time
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

PROJECT_ROOT = Path(__file__).parent.parent


class SignalWaiter:
    """Provides a PID for SIGUSR1 and polls the env file for completion.

    The daemon sends SIGUSR1 to DIRENV_INSTANT_SHELL_PID after it writes
    the env file.  On macOS (especially in nix sandboxes), cross-process
    signal delivery can be unreliable.  Instead of trying to catch the
    signal, we give the daemon a real PID (sleep process) and poll for the
    env file the daemon creates, which is the ground truth for completion.
    """

    def __init__(self) -> None:
        """Start a long-lived sleep process whose PID the daemon will signal."""
        self._proc = subprocess.Popen(
            ["sleep", "3600"],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
        )
        self.pid: int = self._proc.pid

    def wait_for_env_file(self, env_file: Path, timeout: float = 30) -> bool:
        """Poll until *env_file* exists and is non-empty, or timeout."""
        start = time.time()
        while (time.time() - start) < timeout:
            if env_file.exists() and env_file.stat().st_size > 0:
                return True
            time.sleep(0.2)
        return False

    def cleanup(self) -> None:
        """Terminate the sleep process."""
        self._proc.kill()
        self._proc.wait()


class DirenvInstantRunner:
    """Helper to run direnv-instant binary."""

    def __init__(self, binary_path: str) -> None:
        """Initialize with binary path."""
        self.binary_path = binary_path

    def run(
        self, args: list[str], env: dict[str, str]
    ) -> subprocess.CompletedProcess[str]:
        """Run direnv-instant with given args and environment."""
        return subprocess.run(
            [self.binary_path, *args],
            check=False,
            env=env,
            capture_output=True,
            text=True,
        )


@pytest.fixture(scope="session")
def direnv_instant() -> DirenvInstantRunner:
    """Get direnv-instant runner with pre-resolved binary path."""
    if binary := os.environ.get("DIRENV_INSTANT_BIN"):
        # Resolve relative paths against PROJECT_ROOT
        binary_path = Path(binary)
        if not binary_path.is_absolute():
            binary_path = PROJECT_ROOT / binary_path
        return DirenvInstantRunner(str(binary_path.absolute()))

    # Build binary and return the target path
    subprocess.run(
        ["cargo", "build", "--quiet"],
        cwd=PROJECT_ROOT,
        check=True,
        capture_output=True,
    )

    # Find the built binary
    target_dir = PROJECT_ROOT / "target" / "debug"
    binary_name = "direnv-instant.exe" if os.name == "nt" else "direnv-instant"
    binary_path = target_dir / binary_name
    return DirenvInstantRunner(str(binary_path))


@pytest.fixture
def tmux_server() -> Generator[Path]:
    """Set up an isolated tmux server for testing."""
    with tempfile.TemporaryDirectory() as tmpdir:
        socket_path = Path(tmpdir) / "tmux-socket"

        # Start isolated tmux server
        subprocess.run(
            ["tmux", "-S", str(socket_path), "new-session", "-d"],
            check=True,
            capture_output=True,
        )

        try:
            yield socket_path
        finally:
            # Clean up tmux server
            subprocess.run(
                ["tmux", "-S", str(socket_path), "kill-server"],
                check=False,
                capture_output=True,
            )


@pytest.fixture
def signal_waiter() -> Generator[SignalWaiter]:
    """Set up a process that waits for SIGUSR1 signal."""
    waiter = SignalWaiter()
    try:
        yield waiter
    finally:
        waiter.cleanup()


@pytest.fixture
def subprocess_runner() -> Generator[list[subprocess.Popen[str]]]:
    """Manage subprocesses and ensure cleanup."""
    processes: list[subprocess.Popen[str]] = []
    try:
        yield processes
    finally:
        for proc in processes:
            if proc.poll() is None:
                proc.kill()
                proc.wait()
