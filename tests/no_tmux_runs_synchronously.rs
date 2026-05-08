//! Without a multiplexer, `start` blocks and runs direnv synchronously.

mod common;
use common::*;
use std::time::Instant;

#[test]
fn no_tmux_runs_direnv_synchronously() {
    let sb = Sandbox::new("sleep 1\nexport SYNC_TEST=success\n").unwrap();
    let env = sb.base_env();

    let t0 = Instant::now();
    let out = sb.run(&["start"], &env).unwrap();
    let elapsed = t0.elapsed();

    assert!(
        elapsed.as_secs_f64() >= 0.9,
        "returned too quickly: {elapsed:?}"
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stdout.contains("SYNC_TEST") || stderr.contains("SYNC_TEST"),
        "missing export in stdout={stdout:?} stderr={stderr:?}"
    );

    // Synchronous mode does not start a daemon.
    assert!(!stdout.contains("__DIRENV_INSTANT_ENV_FILE"));
    assert!(!stdout.contains("__DIRENV_INSTANT_STDERR_FILE"));
    // Always set so `stop` knows the working dir.
    assert!(stdout.contains("__DIRENV_INSTANT_CURRENT_DIR"));
}
