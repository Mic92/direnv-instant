//! Repeated `start` invocations while a daemon is already evaluating a slow
//! `.envrc` must not leak env.XXXXXX / env_stderr.XXXXXX temp files into the
//! runtime directory (regression test for issue #128).

mod common;
use common::*;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

fn temp_file_count(runtime_dir: &Path) -> usize {
    fs::read_dir(runtime_dir)
        .unwrap()
        .filter_map(|e| e.ok())
        .filter(|e| {
            let name = e.file_name();
            let name = name.to_string_lossy();
            (name.starts_with("env.") && name != "env.stderr") || name.starts_with("env_stderr.")
        })
        .count()
}

#[test]
fn no_temp_leak_when_daemon_running() {
    require!("tmux");
    let sb = Sandbox::new("sleep 3600\n").unwrap();
    let server = TmuxServer::new(&sb.dir).unwrap();
    let sink = SignalSink::new().unwrap();
    let env = sb.tmux_env(&server, sink.pid());

    let out = sb.run(&["start"], &env).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exports = parse_exports(&String::from_utf8_lossy(&out.stdout));
    let runtime_dir = PathBuf::from(&exports["__DIRENV_INSTANT_STDERR_FILE"])
        .parent()
        .unwrap()
        .to_path_buf();
    let socket_path = runtime_dir.join("daemon.sock");

    // Wait until the daemon is up so subsequent starts hit the notify path.
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while !socket_path.exists() {
        assert!(
            std::time::Instant::now() < deadline,
            "daemon socket never appeared"
        );
        std::thread::sleep(Duration::from_millis(100));
    }

    let baseline = temp_file_count(&runtime_dir);

    // Simulate additional shell prompts while the evaluation is still running.
    for _ in 0..3 {
        let out = sb.run(&["start"], &env).unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
    }

    let after = temp_file_count(&runtime_dir);
    assert_eq!(
        after, baseline,
        "temp files leaked into runtime dir: {baseline} -> {after}"
    );

    // Cleanup: stop the daemon.
    let _ = sb.run(&["stop"], &env);
    let _ = wait_for_daemon_exit(&socket_path, Duration::from_secs(5));
}
