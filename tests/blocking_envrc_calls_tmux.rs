//! A blocking `.envrc` triggers a real tmux watch pane, and Ctrl-C in that
//! pane stops the daemon.

mod common;
use common::*;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn blocking_envrc_calls_tmux() {
    require!("tmux");
    let sb = Sandbox::new("sleep 3600\n").unwrap();
    let server = TmuxServer::new(&sb.dir).unwrap();
    let sink = SignalSink::new().unwrap();

    let env = sb.tmux_env(&server, sink.pid());

    let t0 = Instant::now();
    let out = sb.run(&["start"], &env).unwrap();
    assert!(t0.elapsed() < Duration::from_secs(3), "start blocked");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    for needle in [
        "__DIRENV_INSTANT_ENV_FILE",
        "__DIRENV_INSTANT_STDERR_FILE",
        "__DIRENV_INSTANT_CURRENT_DIR",
    ] {
        assert!(stdout.contains(needle), "missing {needle}: {stdout}");
    }

    let exports = parse_exports(&stdout);
    let stderr_file = PathBuf::from(&exports["__DIRENV_INSTANT_STDERR_FILE"]);
    let socket_path = stderr_file.parent().unwrap().join("daemon.sock");

    let pane = server
        .wait_for_watch_pane(Duration::from_secs(5))
        .expect("watch pane never appeared");

    // Ctrl-C in the watch pane should propagate and stop the daemon.
    server.cmd(&["send-keys", "-t", &pane, "C-c"]).unwrap();
    assert!(
        wait_for_daemon_exit(&socket_path, Duration::from_secs(5)),
        "daemon still running after Ctrl-C in watch pane"
    );
}
