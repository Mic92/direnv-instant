//! Fish hook integration: synchronous and async modes.

mod common;
use common::*;
use std::fs;
use std::process::Command;
use std::time::{Duration, Instant};

fn fish_hook() -> String {
    let out = Command::new(bin()).args(["hook", "fish"]).output().unwrap();
    assert!(out.status.success());
    String::from_utf8(out.stdout).unwrap()
}

#[test]
fn fish_runs_direnv_synchronously() {
    require!("fish");
    let sb = Sandbox::new("sleep 1\nexport FISH_TEST=success\n").unwrap();

    let script = format!(
        r#"{hook}
if set -q FISH_TEST
    echo "FISH_TEST=$FISH_TEST"
else
    echo "FISH_TEST not set"
end
"#,
        hook = fish_hook()
    );

    let mut env = sb.base_env();
    env.insert("PATH".into(), prepend_path(&[bin().parent().unwrap()]));

    let t0 = Instant::now();
    let out = Command::new("fish")
        .args(["--no-config", "--command", &script])
        .current_dir(&sb.dir)
        .env_clear()
        .envs(&env)
        .output()
        .unwrap();
    let elapsed = t0.elapsed();

    assert!(
        elapsed.as_secs_f64() >= 0.9,
        "returned too quickly: {elapsed:?}"
    );
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(stdout.contains("FISH_TEST=success"), "{stdout}");
}

#[test]
fn fish_async_creates_env_file() {
    require!("fish");
    let sb = Sandbox::with_envrc(|dir| {
        format!(
            "while [ ! -f {marker} ]; do sleep 0.1; done\nexport ASYNC_TEST=async_success\n",
            marker = dir.join("envrc_done").display()
        )
    })
    .unwrap();
    let done_marker = sb.dir.join("envrc_done");
    sb.write_stub_tmux("exit 0").unwrap();

    let result_file = sb.dir.join("result.txt");
    let script = format!(
        r#"{hook}
echo $__DIRENV_INSTANT_ENV_FILE > {result}
echo unblocking > {marker}
set -l attempts 0
while test $attempts -lt 100
    if test -s "$__DIRENV_INSTANT_ENV_FILE"
        echo "ENV_FILE_READY" >> {result}
        cat "$__DIRENV_INSTANT_ENV_FILE" >> {result}
        exit 0
    end
    sleep 0.1
    set attempts (math $attempts + 1)
end
echo "ENV_FILE_TIMEOUT" >> {result}
"#,
        hook = fish_hook(),
        result = result_file.display(),
        marker = done_marker.display()
    );

    let mut env = sb.async_env(std::process::id(), 1);
    env.insert(
        "PATH".into(),
        prepend_path(&[&sb.dir, bin().parent().unwrap()]),
    );

    let out = Command::new("fish")
        .args(["--no-config", "--command", &script])
        .current_dir(&sb.dir)
        .env_clear()
        .envs(&env)
        .output()
        .unwrap();

    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(
        wait_for_file(&result_file, Duration::from_secs(15)),
        "result file not written"
    );
    let content = fs::read_to_string(&result_file).unwrap();
    assert!(content.contains("ENV_FILE_READY"), "{content}");
    assert!(content.contains("ASYNC_TEST"), "{content}");
    assert!(content.contains("async_success"), "{content}");
    assert!(!content.contains("export "), "got bash syntax: {content}");
}
