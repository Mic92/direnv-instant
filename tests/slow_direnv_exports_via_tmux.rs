//! Slow `.envrc` triggers the multiplexer pane and still exports vars.

mod common;
use common::*;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[test]
fn slow_direnv_exports_via_tmux() {
    let sb = Sandbox::with_envrc(|dir| {
        format!(
            r#"echo "Starting build..." >&2
while [ ! -f {marker} ]; do sleep 0.1; done
echo "Build complete!" >&2
export FOO=bar
export BAZ=qux
"#,
            marker = dir.join("envrc_done").display()
        )
    })
    .unwrap();
    let done_marker = sb.dir.join("envrc_done");
    let tmux_called = sb.dir.join("tmux_called");
    let watch_output = sb.dir.join("watch_output");
    let sink = SignalSink::new().unwrap();

    sb.write_stub_tmux(&format!(
        r#"echo called > {called}
log_path="${{@: -2:1}}"
socket_path="${{@: -1}}"
{bin} watch "$log_path" "$socket_path" > {watch_out} 2>&1 &"#,
        called = tmux_called.display(),
        bin = bin().display(),
        watch_out = watch_output.display()
    ))
    .unwrap();

    let env = sb.async_env(sink.pid(), 1);

    let t0 = Instant::now();
    let out = sb.run(&["start"], &env).unwrap();
    assert!(t0.elapsed() < Duration::from_secs(2), "start blocked");
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );

    let exports = parse_exports(&String::from_utf8_lossy(&out.stdout));
    let env_file = PathBuf::from(&exports["__DIRENV_INSTANT_ENV_FILE"]);

    assert!(
        wait_for_file(&tmux_called, Duration::from_secs(5)),
        "tmux stub was not called"
    );

    fs::write(&done_marker, "go").unwrap();

    assert!(
        wait_for_file(&env_file, Duration::from_secs(30)),
        "env file not created"
    );
    let content = fs::read_to_string(&env_file).unwrap();
    for needle in ["FOO", "bar", "BAZ", "qux"] {
        assert!(content.contains(needle), "missing {needle} in {content}");
    }

    assert!(
        wait_for_file(&watch_output, Duration::from_secs(5)),
        "watch output file empty"
    );
    let w = fs::read_to_string(&watch_output).unwrap();
    assert!(
        w.contains("Starting build") || w.to_lowercase().contains("direnv"),
        "watch output missing direnv output: {w}"
    );
}
