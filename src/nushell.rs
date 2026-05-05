//! Nushell entry point for `start`. The nu hook cannot react to SIGUSR1
//! (no trap primitive) nor `source` dynamic command output (parse-time
//! const path required), so the binary emits a single compact JSON
//! record describing internal var changes and the hook applies it via
//! `load-env` / `hide-env`. Direnv's own export goes to `env_file` as
//! `direnv export json` so the hook can parse it directly.

use crate::daemon::{
    DaemonContext, direnv_export_command, get_runtime_dir, get_socket_path, start_daemon,
    stop_daemon,
};
use crate::mux::Multiplexer;
use crate::shell::Shell;
use std::env;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::Stdio;

pub fn run(parent_pid: i32, find_envrc: impl FnOnce() -> Option<PathBuf>) {
    let direnv = "direnv";
    let mut state: Vec<(&str, Option<String>)> = Vec::new();

    let envrc_dir = match find_envrc() {
        Some(dir) => dir,
        None => {
            state.push(("__DIRENV_INSTANT_CURRENT_DIR", None));
            // direnv emits unset directives for previously-set vars; capture
            // those to a per-pid file so the hook can apply them.
            if let Some(p) = capture_direnv_export(direnv, false, None) {
                state.push(("__DIRENV_INSTANT_ENV_FILE", Some(p.display().to_string())));
            }
            emit_record(&state);
            return;
        }
    };

    if let Ok(current) = env::var("__DIRENV_INSTANT_CURRENT_DIR") {
        let cd = PathBuf::from(&current);
        if cd != envrc_dir {
            stop_daemon(&get_socket_path(&cd));
        }
    }
    state.push((
        "__DIRENV_INSTANT_CURRENT_DIR",
        Some(envrc_dir.display().to_string()),
    ));

    if Multiplexer::detect().is_none() {
        let runtime = get_runtime_dir(&envrc_dir);
        let _ = std::fs::create_dir_all(&runtime);
        let env_file = runtime.join("env");
        if let Some(p) = capture_direnv_export(direnv, true, Some(&env_file)) {
            state.push(("__DIRENV_INSTANT_ENV_FILE", Some(p.display().to_string())));
        }
        emit_record(&state);
        return;
    }

    let ctx = match DaemonContext::new(parent_pid, envrc_dir, Shell::Nushell) {
        Ok(ctx) => ctx,
        Err(e) => {
            eprintln!("direnv-instant: Failed to create temp files: {}", e);
            emit_record(&state);
            return;
        }
    };
    state.push((
        "__DIRENV_INSTANT_ENV_FILE",
        Some(ctx.env_file.display().to_string()),
    ));
    state.push((
        "__DIRENV_INSTANT_STDERR_FILE",
        Some(ctx.stderr_file.display().to_string()),
    ));

    let already_running = ctx.socket_path.exists() && UnixStream::connect(&ctx.socket_path).is_ok();
    emit_record(&state);
    if !already_running {
        start_daemon(direnv, &ctx);
    }
}

fn emit_record(state: &[(&str, Option<String>)]) {
    print!("{{");
    for (i, (k, v)) in state.iter().enumerate() {
        if i > 0 {
            print!(",");
        }
        match v {
            Some(s) => print!(r#""{}":"{}""#, json_escape(k), json_escape(s)),
            None => print!(r#""{}":null"#, json_escape(k)),
        }
    }
    println!("}}");
}

/// Run `direnv export json` and write its stdout to *target*, or to a
/// per-pid temp file if None. Returns the file path on success.
fn capture_direnv_export(
    direnv: &str,
    show_errors: bool,
    target: Option<&Path>,
) -> Option<PathBuf> {
    let mut cmd = direnv_export_command(direnv, Shell::Nushell);
    if !show_errors {
        cmd.stderr(Stdio::null());
    }
    let out = cmd.output().ok()?;
    if !out.status.success() || out.stdout.is_empty() {
        return None;
    }
    let path = match target {
        Some(p) => p.to_path_buf(),
        None => std::env::temp_dir().join(format!("direnv-instant-nu-{}.json", std::process::id())),
    };
    std::fs::write(&path, &out.stdout).ok()?;
    Some(path)
}

/// Minimal JSON string escape (no dep on serde_json).
fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out
}
