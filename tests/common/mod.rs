// Each integration test binary links this module but uses only a subset
// of it; dead-code warnings here are noise, not bugs.
#![allow(dead_code)]

//! Shared test harness for direnv-instant integration tests.
//!
//! Each test runs the real `direnv-instant` binary against a real `direnv`,
//! a real (or stubbed) `tmux`, and a real `.envrc`. Nothing is mocked at
//! the process boundary.

use std::collections::HashMap;
use std::env;
use std::ffi::OsString;
use std::fs;
use std::io;
use std::io::Read;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Output, Stdio};
use std::time::{Duration, Instant};

/// Path to the binary under test.
///
/// `CARGO_BIN_EXE_*` is set by cargo for `[[bin]]` targets when running
/// integration tests. The Nix derivation sets `DIRENV_INSTANT_BIN` instead
/// so the tests can run against the installed binary without cargo.
pub fn bin() -> PathBuf {
    if let Ok(p) = env::var("DIRENV_INSTANT_BIN") {
        return PathBuf::from(p);
    }
    PathBuf::from(env!("CARGO_BIN_EXE_direnv-instant"))
}

/// A throwaway working directory with its own `.envrc`, isolated `HOME`,
/// and a clean environment for child processes.
pub struct Sandbox {
    pub dir: PathBuf,
    pub home: PathBuf,
    _tmp: tempdir::TempDir,
}

impl Sandbox {
    pub fn new(envrc: &str) -> io::Result<Self> {
        Self::with_envrc(|_| envrc.to_owned())
    }

    /// Like [`Self::new`], but the `.envrc` body can reference paths inside
    /// the sandbox dir (e.g. marker files used to gate a slow `.envrc`).
    pub fn with_envrc(f: impl FnOnce(&Path) -> String) -> io::Result<Self> {
        let tmp = tempdir::TempDir::new("direnv-instant-test")?;
        let dir = tmp.path().join("work");
        let home = tmp.path().join("home");
        fs::create_dir_all(&dir)?;
        fs::create_dir_all(&home)?;

        let envrc_path = dir.join(".envrc");
        fs::write(&envrc_path, f(&dir))?;
        fs::set_permissions(&envrc_path, fs::Permissions::from_mode(0o755))?;

        let sb = Self {
            dir,
            home,
            _tmp: tmp,
        };
        sb.allow_direnv()?;
        Ok(sb)
    }

    /// `direnv allow` for the sandbox's `.envrc`. Must be re-run after the
    /// `.envrc` is changed.
    pub fn allow_direnv(&self) -> io::Result<()> {
        let out = Command::new("direnv")
            .arg("allow")
            .current_dir(&self.dir)
            .env("HOME", &self.home)
            .env("XDG_DATA_HOME", self.home.join(".local/share"))
            .output()?;
        assert!(
            out.status.success(),
            "direnv allow failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(())
    }

    /// Base environment for child processes: clean except for what direnv
    /// itself needs. Multiplexer detection variables are stripped so the
    /// caller picks the mode explicitly.
    pub fn base_env(&self) -> HashMap<OsString, OsString> {
        let mut e = HashMap::new();
        e.insert("HOME".into(), self.home.clone().into_os_string());
        e.insert(
            "XDG_DATA_HOME".into(),
            self.home.join(".local/share").into_os_string(),
        );
        e.insert("PATH".into(), env::var_os("PATH").unwrap_or_default());
        // direnv needs a TERM in some sandboxes
        e.insert("TERM".into(), "dumb".into());
        e
    }

    /// Environment for stub-tmux async tests: `TMUX` set, sandbox dir on
    /// PATH so `write_stub_tmux` shadows the real one, short mux delay,
    /// and a real shell PID for the daemon to SIGUSR1.
    pub fn async_env(&self, shell_pid: u32, mux_delay: u32) -> HashMap<OsString, OsString> {
        let mut e = self.base_env();
        e.insert("TMUX".into(), "test".into());
        e.insert(
            "DIRENV_INSTANT_MUX_DELAY".into(),
            mux_delay.to_string().into(),
        );
        e.insert(
            "DIRENV_INSTANT_SHELL_PID".into(),
            shell_pid.to_string().into(),
        );
        e.insert("PATH".into(), prepend_path(&[&self.dir]));
        e
    }

    /// Environment for tests using a real [`TmuxServer`].
    pub fn tmux_env(&self, server: &TmuxServer, shell_pid: u32) -> HashMap<OsString, OsString> {
        let mut e = self.base_env();
        e.insert("TMUX".into(), server.tmux_var());
        e.insert("DIRENV_INSTANT_MUX_DELAY".into(), "1".into());
        e.insert(
            "DIRENV_INSTANT_SHELL_PID".into(),
            shell_pid.to_string().into(),
        );
        e
    }

    /// Run `direnv-instant <args>` in the sandbox dir with the given env.
    pub fn run(&self, args: &[&str], env: &HashMap<OsString, OsString>) -> io::Result<Output> {
        let mut cmd = Command::new(bin());
        cmd.args(args).current_dir(&self.dir).env_clear().envs(env);
        cmd.output()
    }

    /// Spawn `direnv-instant <args>` without waiting.
    pub fn spawn(&self, args: &[&str], env: &HashMap<OsString, OsString>) -> io::Result<Child> {
        let mut cmd = Command::new(bin());
        cmd.args(args)
            .current_dir(&self.dir)
            .env_clear()
            .envs(env)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        cmd.spawn()
    }

    /// Drop a stub `tmux` script into the sandbox dir. Callers must add
    /// `self.dir` to PATH so it shadows the real one.
    pub fn write_stub_tmux(&self, body: &str) -> io::Result<PathBuf> {
        let bash = which("bash").expect("bash on PATH");
        let path = self.dir.join("tmux");
        fs::write(&path, format!("#!{}\n{}\n", bash.display(), body))?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755))?;
        Ok(path)
    }
}

/// Long-lived process whose PID the daemon can SIGUSR1 without affecting
/// the test process. Tests poll for the env file rather than catch the
/// signal because cross-process signal delivery is unreliable in macOS
/// nix sandboxes.
pub struct SignalSink {
    child: Child,
}

impl SignalSink {
    pub fn new() -> io::Result<Self> {
        let child = Command::new("sleep")
            .arg("3600")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()?;
        Ok(Self { child })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }
}

impl Drop for SignalSink {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Isolated tmux server with its own socket. Killed on drop.
pub struct TmuxServer {
    pub socket: PathBuf,
}

impl TmuxServer {
    /// Start with a default-sized session.
    pub fn new(dir: &Path) -> io::Result<Self> {
        Self::with_size(dir, None)
    }

    /// Start with an explicit `(cols, rows)` session size.
    pub fn with_size(dir: &Path, size: Option<(u32, u32)>) -> io::Result<Self> {
        let socket = dir.join("tmux-socket");
        let mut cmd = Command::new("tmux");
        cmd.args(["-S", socket.to_str().unwrap(), "new-session", "-d"]);
        if let Some((x, y)) = size {
            cmd.args(["-x", &x.to_string(), "-y", &y.to_string()]);
        }
        let out = cmd.output()?;
        assert!(
            out.status.success(),
            "tmux new-session failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        Ok(Self { socket })
    }

    /// `$TMUX` value pointing at this server.
    pub fn tmux_var(&self) -> OsString {
        format!("{},0,0", self.socket.display()).into()
    }

    pub fn cmd(&self, args: &[&str]) -> io::Result<Output> {
        Command::new("tmux")
            .args(["-S", self.socket.to_str().unwrap()])
            .args(args)
            .output()
    }

    /// Poll until a pane running `direnv-instant watch` appears, return its id.
    pub fn wait_for_watch_pane(&self, timeout: Duration) -> Option<String> {
        let deadline = Instant::now() + timeout;
        loop {
            if let Ok(out) = self.cmd(&[
                "list-panes",
                "-a",
                "-F",
                "#{pane_id} #{pane_current_command}",
            ]) {
                let stdout = String::from_utf8_lossy(&out.stdout);
                for line in stdout.lines() {
                    if line.contains("direnv-instant") || line.contains("watch") {
                        return line.split_whitespace().next().map(str::to_owned);
                    }
                }
            }
            if Instant::now() >= deadline {
                return None;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    }
}

impl Drop for TmuxServer {
    fn drop(&mut self) {
        let _ = Command::new("tmux")
            .args(["-S", self.socket.to_str().unwrap(), "kill-server"])
            .output();
    }
}

/// Parse `export NAME='value'` lines emitted by `direnv-instant start`.
pub fn parse_exports(stdout: &str) -> HashMap<String, String> {
    let mut m = HashMap::new();
    for line in stdout.lines() {
        let rest = match line.strip_prefix("export ") {
            Some(r) => r,
            None => line,
        };
        if let Some((k, v)) = rest.split_once('=') {
            m.insert(
                k.trim().to_owned(),
                v.trim().trim_matches(|c| c == '\'' || c == '"').to_owned(),
            );
        }
    }
    m
}

/// Poll until `path` exists and is non-empty.
pub fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if path.exists() && fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false) {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Poll until the daemon socket disappears or stops accepting connections.
pub fn wait_for_daemon_exit(socket_path: &Path, timeout: Duration) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if !socket_path.exists() {
            return true;
        }
        if UnixStream::connect(socket_path).is_err() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(Duration::from_millis(200));
    }
}

/// Read a child's stderr until a substring appears or the timeout hits.
///
/// The pipe must be non-blocking so the deadline can fire between reads;
/// otherwise a stalled child that keeps stderr open hangs the test forever.
pub fn read_stderr_until(child: &mut Child, needle: &str, timeout: Duration) -> String {
    let stderr = child.stderr.as_mut().expect("stderr piped");
    set_nonblocking(&*stderr);
    let mut buf = Vec::new();
    let deadline = Instant::now() + timeout;
    let mut chunk = [0u8; 4096];
    loop {
        match stderr.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                buf.extend_from_slice(&chunk[..n]);
                if String::from_utf8_lossy(&buf).contains(needle) {
                    break;
                }
            }
            Err(e) if e.kind() == io::ErrorKind::WouldBlock => {
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
        if Instant::now() >= deadline {
            break;
        }
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn set_nonblocking(fd: &impl std::os::fd::AsFd) {
    use nix::fcntl::{FcntlArg, OFlag, fcntl};
    let flags = OFlag::from_bits_truncate(fcntl(fd, FcntlArg::F_GETFL).unwrap());
    fcntl(fd, FcntlArg::F_SETFL(flags | OFlag::O_NONBLOCK)).unwrap();
}

/// Build a PATH with `extra` prepended to the inherited one.
pub fn prepend_path(extra: &[&Path]) -> OsString {
    let mut parts: Vec<PathBuf> = extra.iter().map(|p| p.to_path_buf()).collect();
    if let Some(p) = env::var_os("PATH") {
        parts.extend(env::split_paths(&p));
    }
    env::join_paths(parts).unwrap()
}

fn which(name: &str) -> Option<PathBuf> {
    env::var_os("PATH").and_then(|paths| {
        env::split_paths(&paths)
            .map(|d| d.join(name))
            .find(|p| p.is_file())
    })
}

/// Skip the test (return early) if a binary isn't available.
#[macro_export]
macro_rules! require {
    ($bin:expr) => {
        if std::env::var_os("PATH")
            .map(|p| {
                std::env::split_paths(&p)
                    .map(|d| d.join($bin))
                    .find(|p| p.is_file())
            })
            .flatten()
            .is_none()
        {
            eprintln!("skipping: {} not on PATH", $bin);
            return;
        }
    };
}

// Tiny TempDir to avoid pulling in the `tempfile` crate.
pub mod tempdir {
    use std::io;
    use std::path::{Path, PathBuf};
    use std::sync::atomic::{AtomicU64, Ordering};

    static COUNTER: AtomicU64 = AtomicU64::new(0);

    pub struct TempDir(PathBuf);

    impl TempDir {
        pub fn new(prefix: &str) -> io::Result<Self> {
            let n = COUNTER.fetch_add(1, Ordering::Relaxed);
            let p = std::env::temp_dir().join(format!("{}-{}-{}", prefix, std::process::id(), n));
            std::fs::create_dir_all(&p)?;
            Ok(Self(p))
        }
        pub fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
}
