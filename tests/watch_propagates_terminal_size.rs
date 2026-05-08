//! Regression test for #49: the watch command must propagate its pane size
//! to the PTY so child programs see the real terminal width.

mod common;
use common::*;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn watch_propagates_terminal_size() {
    require!("tmux");
    let sb = Sandbox::with_envrc(|dir| {
        format!(
            r#"echo "waiting..." >&2
while [ ! -f {marker} ]; do sleep 0.1; done
cols=$(stty size </dev/stderr 2>/dev/null | awk '{{print $2}}')
echo "$cols" > {cols}
echo "cols=$cols" >&2
export DONE=1
"#,
            marker = dir.join("envrc_done").display(),
            cols = dir.join("cols_seen").display()
        )
    })
    .unwrap();
    let done_marker = sb.dir.join("envrc_done");
    let cols_file = sb.dir.join("cols_seen");

    // 200 columns to differ from the 80-column PTY default.
    let server = TmuxServer::with_size(&sb.dir, Some((200, 50))).unwrap();
    let sink = SignalSink::new().unwrap();

    let env = sb.tmux_env(&server, sink.pid());

    let out = sb.run(&["start"], &env).unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let exports = parse_exports(&String::from_utf8_lossy(&out.stdout));
    let env_file = PathBuf::from(&exports["__DIRENV_INSTANT_ENV_FILE"]);

    let pane = server
        .wait_for_watch_pane(Duration::from_secs(10))
        .expect("watch pane never appeared");
    let pane_width: u32 = String::from_utf8_lossy(
        &server
            .cmd(&["display-message", "-t", &pane, "-p", "#{pane_width}"])
            .unwrap()
            .stdout,
    )
    .trim()
    .parse()
    .unwrap();
    assert!(pane_width > 80, "pane only {pane_width} cols wide");

    fs::write(&done_marker, "go").unwrap();

    assert!(
        wait_for_file(&env_file, Duration::from_secs(30)),
        "env file not created"
    );
    let cols_seen: u32 = fs::read_to_string(&cols_file)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        cols_seen, pane_width,
        "child saw {cols_seen} columns but pane is {pane_width} wide"
    );
}
