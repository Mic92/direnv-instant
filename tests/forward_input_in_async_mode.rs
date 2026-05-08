//! Input typed into the watch pane is forwarded to direnv's PTY.

mod common;
use common::*;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn input_forwarded_to_direnv_in_async_mode() {
    require!("tmux");
    let sb = Sandbox::new(
        r#"echo "Enter your name:" >&2
read -r name
export USER_NAME="$name"
echo "Hello, $name!" >&2
"#,
    )
    .unwrap();
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

    let exports = parse_exports(&String::from_utf8_lossy(&out.stdout));
    let env_file = PathBuf::from(&exports["__DIRENV_INSTANT_ENV_FILE"]);

    let pane = server
        .wait_for_watch_pane(Duration::from_secs(5))
        .expect("watch pane never appeared");

    // Give direnv time to print the prompt before we type.
    std::thread::sleep(Duration::from_millis(500));
    server
        .cmd(&["send-keys", "-t", &pane, "Alice", "Enter"])
        .unwrap();

    assert!(
        wait_for_file(&env_file, Duration::from_secs(10)),
        "env file not created"
    );
    let content = fs::read_to_string(&env_file).unwrap();
    assert!(
        content.contains("USER_NAME"),
        "missing USER_NAME: {content}"
    );
    assert!(content.contains("Alice"), "missing Alice: {content}");
}
