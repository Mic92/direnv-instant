//! A second run where direnv exports nothing must not clobber the cached
//! env file from the first run.

mod common;
use common::*;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::Duration;

#[test]
fn empty_export_preserves_cache() {
    let sb = Sandbox::new("export MY_TEST_VAR=hello\n").unwrap();
    sb.write_stub_tmux("exit 0").unwrap();
    let sink = SignalSink::new().unwrap();
    let mut env = sb.async_env(sink.pid(), 1);

    let out1 = sb.run(&["start"], &env).unwrap();
    assert!(
        out1.status.success(),
        "{}",
        String::from_utf8_lossy(&out1.stderr)
    );
    let exports1 = parse_exports(&String::from_utf8_lossy(&out1.stdout));
    let env_file = PathBuf::from(&exports1["__DIRENV_INSTANT_ENV_FILE"]);
    assert!(
        wait_for_file(&env_file, Duration::from_secs(30)),
        "env file not created"
    );
    assert!(
        fs::read_to_string(&env_file)
            .unwrap()
            .contains("MY_TEST_VAR")
    );

    // Pretend the shell applied that env: have bash source `direnv export
    // bash` and dump the resulting environment, then inject it into the
    // second run. This is what the hook would have done after SIGUSR1.
    let dump = Command::new("bash")
        .args(["-c", "eval \"$(direnv export bash)\" && env -0"])
        .current_dir(&sb.dir)
        .env_clear()
        .envs(&env)
        .output()
        .unwrap();
    assert!(
        dump.status.success(),
        "{}",
        String::from_utf8_lossy(&dump.stderr)
    );
    for pair in dump.stdout.split(|b| *b == 0) {
        let pair = String::from_utf8_lossy(pair);
        if let Some((k, v)) = pair.split_once('=') {
            env.insert(k.into(), v.into());
        }
    }

    let sink2 = SignalSink::new().unwrap();
    env.insert(
        "DIRENV_INSTANT_SHELL_PID".into(),
        sink2.pid().to_string().into(),
    );
    let out2 = sb.run(&["start"], &env).unwrap();
    assert!(
        out2.status.success(),
        "{}",
        String::from_utf8_lossy(&out2.stderr)
    );

    // Daemon emits no SIGUSR1; just wait for it to exit.
    let socket_path = env_file.parent().unwrap().join("daemon.sock");
    wait_for_daemon_exit(&socket_path, Duration::from_secs(10));

    assert!(
        fs::read_to_string(&env_file)
            .unwrap()
            .contains("MY_TEST_VAR"),
        "env file was clobbered by empty export"
    );
}
