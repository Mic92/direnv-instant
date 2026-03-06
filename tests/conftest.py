"""Pytest configuration and fixtures for direnv-instant tests."""

from __future__ import annotations

import os
import subprocess
import tempfile
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

if TYPE_CHECKING:
    from collections.abc import Generator

PROJECT_ROOT = Path(__file__).parent.parent

# Helper script spawned as a subprocess to wait for SIGUSR1.
# Using a subprocess instead of multiprocessing.Process avoids
# the DeprecationWarning for fork() in multi-threaded processes
# (Python 3.13.4+ defaults to 'spawn' on macOS) and sidesteps
# issues where the signal handler is not installed in time.
_SIGNAL_WAITER_SCRIPT = """\
import os, signal, sys, time

# Write PID so the parent can read it
sys.stdout.write(str(os.getpid()) + '\\n')
sys.stdout.flush()

received = False

def handler(signum, frame):
    global received
    received = True

signal.signal(signal.SIGUSR1, handler)

# Notify parent that the handler is installed
sys.stdout.write('READY\\n')
sys.stdout.flush()

# Poll until signal received or timeout (read from stdin for shutdown)
start = time.monotonic()
timeout = 30
while not received and (time.monotonic() - start) < timeout:
    time.sleep(0.1)

sys.exit(0 if received else 1)
"""


class SignalWaiter:
    """Waits for SIGUSR1 signal in a subprocess."""

    def __init__(self) -> None:
        """Initialize signal waiter as a plain subprocess."""
        self._process = subprocess.Popen(
            ["python3", "-c", _SIGNAL_WAITER_SCRIPT],
            stdin=subprocess.PIPE,
            stdout=subprocess.PIPE,
            text=True,
        )
        assert self._process.stdout is not None

        # Read the PID line
        pid_line = self._process.stdout.readline().strip()
        self.pid = int(pid_line)

        # Wait for READY to ensure signal handler is installed
        ready_line = self._process.stdout.readline().strip()
        assert ready_line == "READY", f"Expected READY, got {ready_line!r}"

    def wait(self, timeout: float = 30) -> bool:
        """Wait for signal and return whether it was received."""
        try:
            self._process.wait(timeout=timeout)
        except subprocess.TimeoutExpired:
            self._process.kill()
            self._process.wait()
            return False
        return self._process.returncode == 0

    def cleanup(self) -> None:
        """Clean up the signal process."""
        if self._process.poll() is None:
            self._process.kill()
            self._process.wait()


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
