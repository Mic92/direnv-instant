//! Regression test for #7: SIGINT must cancel the synchronous direnv run,
//! not restart it.

mod common;
use common::*;
use nix::sys::signal::{Signal, kill};
use nix::unistd::Pid;
use std::io::Read;
use std::time::{Duration, Instant};

#[test]
fn ctrl_c_cancels_direnv_in_non_mux_mode() {
    let sb = Sandbox::new(
        r#"for i in $(seq 1 10); do
    sleep 1
    echo "Processing step $i..." >&2
done
export SLOW_TEST=completed
"#,
    )
    .unwrap();

    // No multiplexer vars => synchronous mode.
    let env = sb.base_env();
    let mut child = sb.spawn(&["start"], &env).unwrap();

    // Confirm direnv started before we interrupt it.
    let early = read_stderr_until(&mut child, "Processing step", Duration::from_secs(5));
    assert!(
        early.contains("Processing step"),
        "direnv never started: {early}"
    );

    kill(Pid::from_raw(child.id() as i32), Signal::SIGINT).unwrap();

    let t0 = Instant::now();
    let deadline = t0 + Duration::from_secs(8);
    let status = loop {
        if let Some(s) = child.try_wait().unwrap() {
            break s;
        }
        if Instant::now() >= deadline {
            child.kill().ok();
            child.wait().ok();
            panic!("did not terminate within 8s after SIGINT");
        }
        std::thread::sleep(Duration::from_millis(50));
    };

    let mut stdout = String::new();
    child
        .stdout
        .take()
        .unwrap()
        .read_to_string(&mut stdout)
        .ok();

    assert!(!status.success(), "expected non-zero exit after SIGINT");
    assert!(
        !stdout.contains("SLOW_TEST"),
        "operation completed despite SIGINT: {stdout}"
    );
}
