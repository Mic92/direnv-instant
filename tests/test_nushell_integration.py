"""Test direnv-instant integration with Nushell.

Nushell has no signal-handler primitive, so the hook can't react to the
daemon's SIGUSR1. It instead loads the env_file (direnv's `export json`
output) at pre_prompt and pre_execution. This test exercises the sync
no-mux path (which requires the JSON-emitting nushell mode in `start`)
and verifies env apply, unset on leave, and re-apply on re-enter.
"""

from __future__ import annotations

import os
import shutil
import subprocess
from pathlib import Path
from typing import TYPE_CHECKING

import pytest

from tests.helpers import allow_direnv, setup_envrc

if TYPE_CHECKING:
    from _pytest.monkeypatch import MonkeyPatch

    from tests.conftest import DirenvInstantRunner


NU = shutil.which("nu")
pytestmark = pytest.mark.skipif(NU is None, reason="nushell not installed")


def test_nushell_apply_unset_reenter(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    direnv_instant: DirenvInstantRunner,
) -> None:
    setup_envrc(tmp_path, "export NU_TEST_VAR=hello_from_direnv\n")
    allow_direnv(tmp_path, monkeypatch)

    binary_dir = Path(direnv_instant.binary_path).resolve().parent
    hook_path = Path(__file__).resolve().parent.parent / "hooks" / "nushell.nu"
    script = tmp_path / "drive.nu"
    script.write_text(
        f"""$env.PATH = ([
    "{binary_dir}"
    (which direnv | get path | first | path dirname)
] | append ($env.PATH | split row (char esep)))
source {hook_path}

cd {tmp_path}
_direnv_instant_hook
print $"enter=($env.NU_TEST_VAR? | default '<unset>')"

cd /
_direnv_instant_hook
print $"leave=($env.NU_TEST_VAR? | default '<unset>')"

cd {tmp_path}
_direnv_instant_hook
print $"reenter=($env.NU_TEST_VAR? | default '<unset>')"
"""
    )

    # Minimal env: avoid inheriting parent direnv/multiplexer state that
    # would make `direnv export json` emit unsets for the parent env (and
    # in particular clobber PATH on apply).
    env = {
        "HOME": os.environ["HOME"],
        "PATH": os.environ["PATH"],
        "XDG_DATA_HOME": os.environ.get(
            "XDG_DATA_HOME", str(Path(os.environ["HOME"]) / ".local/share")
        ),
    }
    result = subprocess.run(
        [NU, str(script)],
        check=False,
        capture_output=True,
        text=True,
        env=env,
    )
    assert result.returncode == 0, (
        f"nu exited {result.returncode}\n"
        f"--stdout--\n{result.stdout}\n--stderr--\n{result.stderr}"
    )
    out = result.stdout
    assert "enter=hello_from_direnv" in out, out
    assert "leave=<unset>" in out, out
    assert "reenter=hello_from_direnv" in out, out
