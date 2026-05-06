"""Regression test for #88: user PATH additions must survive subsequent prompts.

Reproduction:
- Enter an envrc dir (direnv-instant loads envrc).
- User runs `export PATH=/userdir:$PATH` (mimicking `nix shell`'s effect).
- Next prompt fires the precmd hook.
- /userdir must still be on PATH afterwards.

Pre-fix the hook re-eval'd the cached env_file every prompt, which contains
`export PATH='<original>'` and clobbered /userdir. Now we delegate to
`direnv export` directly when the envrc is already loaded — direnv emits
only the delta and leaves user additions alone.
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


ZSH = shutil.which("zsh")
pytestmark = pytest.mark.skipif(ZSH is None, reason="zsh not installed")


def test_user_path_addition_survives_next_prompt(
    tmp_path: Path,
    monkeypatch: MonkeyPatch,
    direnv_instant: DirenvInstantRunner,
) -> None:
    setup_envrc(tmp_path, "PATH_add /tmp/dir-from-envrc\nexport FROM_ENVRC=1\n")
    allow_direnv(tmp_path, monkeypatch)

    binary_dir = Path(direnv_instant.binary_path).resolve().parent
    user_dir = tmp_path / "user-bin"
    user_dir.mkdir()

    # Bug only manifests in multiplexer/daemon mode (the cached env_file
    # lives there); fake a TMUX session to take that code path.
    fake_tmux_state = tmp_path / "fake-tmux"
    fake_tmux_state.touch()

    # Each line in the script triggers a precmd cycle in `zsh -i < script`.
    # Between the user's `export PATH=...` line and the inspection line,
    # precmd fires `_direnv_hook`. The MARKER must contain user_dir.
    script = tmp_path / "drive.zsh"
    script.write_text(
        f'eval "$(direnv-instant hook zsh)"\n'
        f"export PATH={user_dir}:$PATH\n"
        f'echo "MARKER=$PATH"\n'
    )

    env = {
        "HOME": os.environ["HOME"],
        "PATH": f"{binary_dir}:{os.environ['PATH']}",
        "XDG_DATA_HOME": os.environ.get(
            "XDG_DATA_HOME", str(Path(os.environ["HOME"]) / ".local/share")
        ),
        "TMUX": f"{fake_tmux_state},0,0",
        "DIRENV_INSTANT_MUX_DELAY": "60000",
    }
    result = subprocess.run(
        [ZSH, "-i"],
        check=False,
        input=script.read_text(),
        capture_output=True,
        text=True,
        env=env,
        cwd=str(tmp_path),
    )

    # zsh -i decorates output with terminal escape sequences; locate the
    # MARKER substring anywhere in the joined stdout.
    idx = result.stdout.find("MARKER=")
    assert idx != -1, (
        f"MARKER not found in stdout:\n{result.stdout}\n--stderr--\n{result.stderr}"
    )
    marker = result.stdout[idx : idx + 4096].splitlines()[0]
    assert str(user_dir) in marker, (
        f"User PATH addition was clobbered by precmd re-eval (issue #88).\n"
        f"Got: {marker}"
    )
