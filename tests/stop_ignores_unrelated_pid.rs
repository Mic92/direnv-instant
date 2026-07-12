//! Regression test for https://github.com/Mic92/direnv-instant/issues/130:
//! `stop` from a shell that never registered with the daemon (e.g. a
//! short-lived `fish -c` child inheriting the hook's exported vars) must not
//! kill the daemon its parent shell is still waiting on.

mod common;
use common::*;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn stop_from_unregistered_pid_keeps_daemon_alive() {
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

    env.insert(
        "__DIRENV_INSTANT_CURRENT_DIR".into(),
        exports["__DIRENV_INSTANT_CURRENT_DIR"].clone().into(),
    );

    // A pid that never sent NOTIFY to this daemon (simulates a `fish -c`
    // child whose exit hook fires). PID 1 is guaranteed to exist and never
    // be one of ours.
    let mut child_env = env.clone();
    child_env.insert("DIRENV_INSTANT_SHELL_PID".into(), "1".into());
    let stop = sb.run(&["stop"], &child_env).unwrap();
    assert!(
        stop.status.success(),
        "{}",
        String::from_utf8_lossy(&stop.stderr)
    );

    // Daemon must still be reachable.
    std::thread::sleep(Duration::from_millis(300));
    assert!(
        std::os::unix::net::UnixStream::connect(&socket_path).is_ok(),
        "daemon died after stop from an unregistered pid"
    );

    // Stop from the registered shell pid still shuts it down.
    let stop = sb.run(&["stop"], &env).unwrap();
    assert!(stop.status.success());
    assert!(
        wait_for_daemon_exit(&socket_path, Duration::from_secs(15)),
        "daemon still running after stop from owning shell"
    );
}
