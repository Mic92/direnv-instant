"""Regression test for #105: select() EINTR must not kill watch loop."""

from __future__ import annotations

import os
import pty
import signal
import socket
import struct
import subprocess
import time
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

    from tests.conftest import DirenvInstantRunner


def test_watch_survives_sigwinch(
    tmp_path: Path,
    direnv_instant: DirenvInstantRunner,
) -> None:
    log_path = tmp_path / "watch.log"
    log_path.touch()
    socket_path = tmp_path / "daemon.sock"

    # Watch needs a TTY stdin to request the PTY fd from the daemon.
    master_fd, slave_fd = pty.openpty()

    daemon_sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    daemon_sock.bind(str(socket_path))
    daemon_sock.listen(2)

    watch_proc = subprocess.Popen(
        [direnv_instant.binary_path, "watch", str(log_path), str(socket_path)],
        stdin=slave_fd,
        stdout=subprocess.DEVNULL,
        stderr=subprocess.DEVNULL,
    )
    os.close(slave_fd)
    monitoring_conn: socket.socket | None = None
    try:
        # WATCH request → reply with PTY master fd over SCM_RIGHTS.
        watch_conn, _ = daemon_sock.accept()
        with watch_conn:
            assert watch_conn.recv(64).startswith(b"WATCH")
            watch_conn.sendmsg(
                [b"OK\n"],
                [(socket.SOL_SOCKET, socket.SCM_RIGHTS, struct.pack("i", master_fd))],
            )
        # Long-lived monitoring connection; watch exits when daemon closes it.
        monitoring_conn, _ = daemon_sock.accept()

        time.sleep(0.5)
        assert watch_proc.poll() is None, "watch exited before SIGWINCH"

        os.kill(watch_proc.pid, signal.SIGWINCH)
        time.sleep(0.5)

        assert watch_proc.poll() is None, (
            f"watch exited after SIGWINCH (rc={watch_proc.returncode})"
        )
    finally:
        if monitoring_conn is not None:
            monitoring_conn.close()
        daemon_sock.close()
        os.close(master_fd)
        try:
            watch_proc.wait(timeout=5)
        except subprocess.TimeoutExpired:
            watch_proc.kill()
            watch_proc.wait()
