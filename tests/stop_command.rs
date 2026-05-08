//! `stop` terminates the daemon and removes its socket.

mod common;
use common::*;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn stop_command_stops_daemon() {
    let sb = Sandbox::new("sleep 3600\n").unwrap();
    sb.write_stub_tmux("exit 0").unwrap();

    // Long mux delay so the daemon never tries to spawn the watch pane.
    let mut env = sb.async_env(std::process::id(), 60);

    let out = sb.run(&["start"], &env).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let exports = parse_exports(&String::from_utf8_lossy(&out.stdout));
    let stderr_file = PathBuf::from(&exports["__DIRENV_INSTANT_STDERR_FILE"]);
    let socket_path = stderr_file.parent().unwrap().join("daemon.sock");

    let mut waited = Duration::ZERO;
    while !socket_path.exists() && waited < Duration::from_secs(3) {
        std::thread::sleep(Duration::from_millis(100));
        waited += Duration::from_millis(100);
    }
    assert!(socket_path.exists(), "daemon socket never appeared");

    // `stop` derives the daemon socket from __DIRENV_INSTANT_CURRENT_DIR.
    // Use the value `start` emitted, not sb.dir: on macOS /tmp is a symlink
    // and the canonicalized path hashes to a different runtime directory.
    env.insert(
        "__DIRENV_INSTANT_CURRENT_DIR".into(),
        exports["__DIRENV_INSTANT_CURRENT_DIR"].clone().into(),
    );
    let stop = sb.run(&["stop"], &env).unwrap();
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );

    assert!(
        wait_for_daemon_exit(&socket_path, Duration::from_secs(15)),
        "daemon still running after stop"
    );
}
