use crate::daemon::{
    DaemonContext, direnv_export_command, get_socket_path, notify_daemon, start_daemon, stop_daemon,
};
use crate::mux::Multiplexer;
use crate::shell::Shell;
use nix::unistd::getppid;
use std::env;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub fn run() {
    let direnv = "direnv";
    let shell = Shell::from_env();
    let parent_pid = env::var("DIRENV_INSTANT_SHELL_PID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| getppid().as_raw());

    // Find .envrc directory
    let envrc_dir = match find_envrc() {
        Some(dir) => dir,
        None => {
            shell.unset_var("__DIRENV_INSTANT_CURRENT_DIR");
            run_direnv_sync(direnv, shell, false);
            return;
        }
    };

    // Steady-state fast path: envrc is already loaded into the current shell
    // (DIRENV_DIR matches and __DIRENV_INSTANT_CURRENT_DIR is unchanged), so
    // delegate to `direnv export` directly. It emits only the delta needed
    // and preserves user PATH additions (e.g. from `nix shell`) instead of
    // re-applying the cached snapshot wholesale (issue #88).
    if envrc_already_loaded(&envrc_dir)
        && env::var("__DIRENV_INSTANT_CURRENT_DIR").as_deref()
            == Ok(&envrc_dir.display().to_string())
    {
        run_direnv_sync(direnv, shell, true);
        return;
    }

    // Check if we changed dirs since the last hook fire. The cache emit
    // below is gated on this so the user's mid-prompt env mutations aren't
    // clobbered on every prompt (issue #88).
    let dir_changed = match env::var("__DIRENV_INSTANT_CURRENT_DIR") {
        Ok(current) => {
            let current_dir = PathBuf::from(&current);
            if current_dir != envrc_dir {
                stop_daemon(&get_socket_path(&current_dir));
                true
            } else {
                false
            }
        }
        Err(_) => true,
    };
    shell.export_var(
        "__DIRENV_INSTANT_CURRENT_DIR",
        &envrc_dir.display().to_string(),
    );

    // If not in a multiplexer, just run direnv synchronously
    if Multiplexer::detect().is_none() {
        run_direnv_sync(direnv, shell, true);
        return;
    }

    // Set up daemon context
    let ctx = match DaemonContext::new(parent_pid, envrc_dir, shell) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("direnv-instant: Failed to create temp files: {}", e);
            run_direnv_sync(direnv, shell, true);
            return;
        }
    };
    shell.export_var(
        "__DIRENV_INSTANT_ENV_FILE",
        &ctx.env_file.display().to_string(),
    );
    shell.export_var(
        "__DIRENV_INSTANT_STDERR_FILE",
        &ctx.stderr_file.display().to_string(),
    );

    // Cold-start UX: when entering this dir, emit the cached env_file once
    // so the prompt sees the env immediately. Subsequent prompts in the
    // same dir skip this so user-added env vars aren't clobbered. The
    // daemon will refresh asynchronously and SIGUSR1 the shell when it has
    // fresh output.
    if dir_changed
        && ctx.env_file.exists()
        && let Ok(content) = std::fs::read_to_string(&ctx.env_file)
    {
        print!("{}", content);
    }

    // Check if daemon is already running
    if ctx.socket_path.exists() && notify_daemon(&ctx.socket_path, parent_pid) {
        return;
    }

    start_daemon(direnv, &ctx);
}

/// True if the current shell already has *envrc_dir* loaded by direnv (i.e.
/// `DIRENV_DIR == "-<envrc_dir>"`, the format direnv uses internally).
fn envrc_already_loaded(envrc_dir: &Path) -> bool {
    let Ok(direnv_dir) = env::var("DIRENV_DIR") else {
        return false;
    };
    let stripped = direnv_dir.strip_prefix('-').unwrap_or(&direnv_dir);
    Path::new(stripped) == envrc_dir
}

fn find_envrc() -> Option<PathBuf> {
    let mut dir = env::current_dir().ok()?;
    loop {
        if dir.join(".envrc").exists() {
            return Some(dir);
        }
        if !dir.pop() {
            return None;
        }
    }
}

fn run_direnv_sync(direnv: &str, shell: Shell, show_errors: bool) {
    let mut cmd = direnv_export_command(direnv, shell);
    if !show_errors {
        cmd.stderr(Stdio::null());
    }

    let err = cmd.exec();

    eprintln!("direnv-instant: Failed to exec direnv: {}", err);
    std::process::exit(1);
}
