// Regression test for https://github.com/Mic92/direnv-instant/issues/129:
// `direnv-instant start` must not panic with "Broken pipe" when the reader
// of its stdout (the shell hook's `source` pipeline) goes away early.

mod common;

use std::process::Command;

#[test]
fn start_does_not_panic_when_stdout_reader_closes_early() {
    let sb = common::Sandbox::new("export FOO=bar").unwrap();
    let env = sb.base_env();

    // `head -c 0` closes the read end immediately, so every write to stdout
    // from `start` hits EPIPE.
    let out = Command::new("bash")
        .arg("-c")
        .arg(format!("'{}' start | head -c 0", common::bin().display()))
        .current_dir(&sb.dir)
        .env_clear()
        .envs(&env)
        .env("DIRENV_INSTANT_SHELL", "fish")
        .env("DIRENV_INSTANT_SHELL_PID", std::process::id().to_string())
        .output()
        .unwrap();

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        !stderr.contains("panicked"),
        "start panicked on broken pipe:\n{}",
        stderr
    );
}
