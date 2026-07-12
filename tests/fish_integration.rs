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

/// Regression test for issue #131: after the async replay the handler must
/// repaint the prompt, otherwise the shell looks hung until ENTER.
#[test]
fn fish_repaints_prompt_after_async_replay() {
    use nix::pty::openpty;
    use std::io::Read;
    use std::os::unix::process::CommandExt;
    use std::process::Stdio;
    use std::sync::{Arc, Mutex};

    require!("fish");
    let sb = Sandbox::with_envrc(|dir| {
        format!(
            "while [ ! -f {marker} ]; do sleep 0.1; done\n\
             echo hello_from_direnv >&2\n\
             export ASYNC_TEST=1\n",
            marker = dir.join("envrc_done").display()
        )
    })
    .unwrap();
    let done_marker = sb.dir.join("envrc_done");
    sb.write_stub_tmux("exit 0").unwrap();

    // Custom prompt so we can detect every prompt paint in the pty output.
    let init = sb.dir.join("init.fish");
    fs::write(
        &init,
        format!(
            "{hook}\nfunction fish_prompt\n    echo PROMPT_MARK\nend\n",
            hook = fish_hook()
        ),
    )
    .unwrap();

    let mut env = sb.async_env(std::process::id(), 1);
    env.insert(
        "PATH".into(),
        prepend_path(&[&sb.dir, bin().parent().unwrap()]),
    );

    let pty = openpty(None, None).unwrap();
    let slave = pty.slave;
    let master = pty.master;

    let mut cmd = Command::new("fish");
    cmd.args([
        "--no-config",
        "-i",
        "-C",
        &format!("source {}", init.display()),
    ])
    .current_dir(&sb.dir)
    .env_clear()
    .envs(&env)
    .stdin(Stdio::from(slave.try_clone().unwrap()))
    .stdout(Stdio::from(slave.try_clone().unwrap()))
    .stderr(Stdio::from(slave));
    // Make the pty the controlling terminal so fish runs fully interactive.
    unsafe {
        cmd.pre_exec(|| {
            nix::unistd::setsid().map_err(std::io::Error::from)?;
            if nix::libc::ioctl(0, nix::libc::TIOCSCTTY as _, 0) < 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().unwrap();

    // Drain the pty master into a shared buffer.
    let buf = Arc::new(Mutex::new(String::new()));
    let buf_writer = buf.clone();
    let mut master_file = std::fs::File::from(master);
    std::thread::spawn(move || {
        let mut chunk = [0u8; 4096];
        while let Ok(n) = master_file.read(&mut chunk) {
            if n == 0 {
                break;
            }
            buf_writer
                .lock()
                .unwrap()
                .push_str(&String::from_utf8_lossy(&chunk[..n]));
        }
    });

    let wait_for_output = |pred: &dyn Fn(&str) -> bool, timeout: Duration| -> String {
        let deadline = Instant::now() + timeout;
        loop {
            let snapshot = buf.lock().unwrap().clone();
            if pred(&snapshot) {
                return snapshot;
            }
            if Instant::now() >= deadline {
                return snapshot;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };

    // Wait for the first prompt, then let the .envrc finish.
    let out = wait_for_output(&|s| s.contains("PROMPT_MARK"), Duration::from_secs(15));
    assert!(out.contains("PROMPT_MARK"), "no initial prompt: {out}");
    fs::write(&done_marker, "go").unwrap();

    // Wait for the async replay of direnv's stderr.
    let out = wait_for_output(
        &|s| s.contains("hello_from_direnv"),
        Duration::from_secs(15),
    );
    let replay_pos = out
        .find("hello_from_direnv")
        .unwrap_or_else(|| panic!("no replay: {out}"));

    // A fresh prompt must be painted after the replay.
    let out = wait_for_output(
        &|s| s[replay_pos..].contains("PROMPT_MARK"),
        Duration::from_secs(10),
    );
    let _ = child.kill();
    let _ = child.wait();
    assert!(
        out[replay_pos..].contains("PROMPT_MARK"),
        "prompt not repainted after async replay: {out}"
    );
}
