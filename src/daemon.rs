use nix::errno::Errno;
use nix::libc;
use nix::pty::{ForkptyResult, Winsize, forkpty};
use nix::sys::select::{FdSet, select};
use nix::sys::signal::{Signal, kill};
use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg};
use nix::sys::time::{TimeVal, TimeValLike};
use nix::sys::wait::{WaitPidFlag, WaitStatus, waitpid};
use nix::unistd::{ForkResult, Pid, dup2_stderr, dup2_stdin, dup2_stdout, fork, read, setsid};
use std::collections::hash_map::DefaultHasher;
use std::ffi::OsString;
use std::fs::{File, remove_file};
use std::hash::{Hash, Hasher};
use std::io::{BufRead, BufReader, IoSlice, Write};
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::{env, thread};

use crate::mux::{self, Multiplexer};

const PTY_WINSIZE: Winsize = Winsize {
    ws_row: 24,
    ws_col: 80,
    ws_xpixel: 0,
    ws_ypixel: 0,
};

pub fn get_runtime_dir(envrc_dir: &Path) -> PathBuf {
    let mut hasher = DefaultHasher::new();
    envrc_dir.hash(&mut hasher);
    let dir_hash = hasher.finish();

    let cache_base = env::var("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            env::var("HOME")
                .map(|h| PathBuf::from(h).join(".cache"))
                .unwrap_or_else(|_| PathBuf::from("/tmp"))
        });

    cache_base
        .join("direnv-instant")
        .join(format!("{:x}", dir_hash))
}

pub fn get_socket_path(envrc_dir: &Path) -> PathBuf {
    get_runtime_dir(envrc_dir).join("daemon.sock")
}

fn create_temp_file(runtime_dir: &Path, prefix: &str) -> std::io::Result<PathBuf> {
    let template = runtime_dir.join(format!("{}.XXXXXX", prefix));
    let mut bytes = template.into_os_string().into_vec();
    bytes.push(0); // null terminator

    let fd = unsafe { libc::mkstemp(bytes.as_mut_ptr().cast()) };
    if fd == -1 {
        return Err(std::io::Error::last_os_error());
    }

    // Close the fd immediately - daemon will reopen and handle cleanup
    unsafe { libc::close(fd) };

    bytes.pop(); // remove null terminator
    Ok(PathBuf::from(OsString::from_vec(bytes)))
}

pub struct DaemonContext {
    pub parent_pid: i32,
    pub socket_path: PathBuf,
    pub env_file: PathBuf,
    pub stderr_file: PathBuf,
    pub temp_file: PathBuf,
    pub temp_stderr: PathBuf,
    pub multiplexer: Option<Multiplexer>,
}

impl DaemonContext {
    pub fn new(parent_pid: i32, envrc_dir: PathBuf) -> std::io::Result<Self> {
        let runtime_dir = get_runtime_dir(&envrc_dir);

        // Create runtime directory if it doesn't exist (needed for mkstemp)
        std::fs::create_dir_all(&runtime_dir)?;
        // Ensure owner-only permissions even if directory already exists
        std::fs::set_permissions(&runtime_dir, PermissionsExt::from_mode(0o700))?;

        let temp_file = create_temp_file(&runtime_dir, "env")?;
        let temp_stderr = create_temp_file(&runtime_dir, "env_stderr")?;

        Ok(Self {
            parent_pid,
            socket_path: runtime_dir.join("daemon.sock"),
            env_file: runtime_dir.join("env"),
            stderr_file: runtime_dir.join("env.stderr"),
            temp_file,
            temp_stderr,
            multiplexer: Multiplexer::detect(),
        })
    }
}

struct Cleanup<'a>(&'a DaemonContext);
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        let ctx = self.0;
        let _ = remove_file(&ctx.socket_path);
        // Clean up temp files if they weren't renamed
        let _ = remove_file(&ctx.temp_file);
        let _ = remove_file(&ctx.temp_stderr);
    }
}

fn send_daemon_message(socket_path: &Path, message: &str) -> std::io::Result<()> {
    UnixStream::connect(socket_path).and_then(|mut stream| stream.write_all(message.as_bytes()))
}

pub fn notify_daemon(socket_path: &Path, shell_pid: i32) -> bool {
    send_daemon_message(socket_path, &format!("NOTIFY {shell_pid}\n")).is_ok()
}

pub fn stop_daemon(socket_path: &Path) {
    let _ = send_daemon_message(socket_path, "STOP\n");
}

pub fn start_daemon(direnv_cmd: &str, ctx: &DaemonContext) {
    // Check if daemon already running
    if ctx.socket_path.exists() {
        if UnixStream::connect(&ctx.socket_path).is_ok() {
            return; // Already running
        }
        let _ = remove_file(&ctx.socket_path); // Stale socket
    }

    match unsafe { fork() } {
        Ok(ForkResult::Parent { child }) => {
            let _ = waitpid(child, None);
        }
        Ok(ForkResult::Child) => {
            setsid().expect("Failed to setsid");
            // Double fork to fully daemonize
            match unsafe { fork() } {
                Ok(ForkResult::Parent { .. }) => std::process::exit(0),
                Ok(ForkResult::Child) => {
                    // Redirect stdin, stdout, stderr to detach from parent
                    let devnull = File::open("/dev/null").expect("Failed to open /dev/null");
                    dup2_stdin(&devnull).expect("Failed to redirect stdin");

                    // For debugging, allow redirecting to a log file instead of /dev/null
                    if let Ok(debug_log) = env::var("DIRENV_INSTANT_DEBUG_LOG") {
                        if let Ok(logfile) = File::create(&debug_log) {
                            dup2_stdout(&logfile).ok();
                            dup2_stderr(&logfile).ok();
                        }
                    } else {
                        dup2_stdout(&devnull).expect("Failed to redirect stdout");
                        dup2_stderr(&devnull).expect("Failed to redirect stderr");
                    }

                    run_direnv(direnv_cmd, ctx);
                }
                Err(e) => {
                    eprintln!("direnv-instant: Second fork failed: {}", e);
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("direnv-instant: First fork failed: {}", e);
            std::process::exit(1);
        }
    }
}

pub fn direnv_export_command(direnv_cmd: &str) -> Command {
    let mut cmd = Command::new(direnv_cmd);
    // TODO: support other shells
    cmd.args(["export", "fish"]);
    cmd
}

fn handle_socket_commands(
    listener: UnixListener,
    notify_pids: Arc<Mutex<Vec<i32>>>,
    should_stop: Arc<AtomicBool>,
    pty_master: Arc<Mutex<Option<OwnedFd>>>,
) {
    for stream in listener.incoming().flatten() {
        let mut reader = BufReader::new(&stream);
        let mut line = String::new();
        if reader.read_line(&mut line).is_ok() {
            if let Some(stripped) = line.strip_prefix("NOTIFY ") {
                if let Ok(pid) = stripped.trim().parse::<i32>() {
                    notify_pids.lock().expect("Failed to lock").push(pid);
                }
            } else if line.starts_with("STOP") {
                should_stop.store(true, Ordering::Relaxed);
                break;
            } else if line.starts_with("WATCH") {
                // Send PTY master fd to watch command via SCM_RIGHTS
                if let Some(ref owned_fd) = *pty_master.lock().expect("Failed to lock") {
                    let iov = [IoSlice::new(b"OK\n")];
                    let fds = [owned_fd.as_raw_fd()];
                    let cmsg = ControlMessage::ScmRights(&fds);
                    if let Err(e) =
                        sendmsg::<()>(stream.as_raw_fd(), &iov, &[cmsg], MsgFlags::empty(), None)
                    {
                        eprintln!(
                            "direnv-instant: Failed to send PTY fd to WATCH client: {}",
                            e
                        );
                    }
                } else {
                    // PTY master not available, send error response
                    eprintln!("direnv-instant: WATCH requested but PTY master not available");
                    let iov = [IoSlice::new(b"ERR\n")];
                    if let Err(e) =
                        sendmsg::<()>(stream.as_raw_fd(), &iov, &[], MsgFlags::empty(), None)
                    {
                        eprintln!(
                            "direnv-instant: Failed to send error response to WATCH client: {}",
                            e
                        );
                    }
                }
            }
        }
    }
}

fn run_direnv(direnv_cmd: &str, ctx: &DaemonContext) {
    let _cleanup = Cleanup(ctx);

    let listener = UnixListener::bind(&ctx.socket_path).expect("Failed to bind socket");
    let notify_pids = Arc::new(Mutex::new(vec![ctx.parent_pid]));
    let should_stop = Arc::new(AtomicBool::new(false));
    let pty_master: Arc<Mutex<Option<OwnedFd>>> = Arc::new(Mutex::new(None));

    let notify_clone = notify_pids.clone();
    let stop_clone = should_stop.clone();
    let pty_clone = pty_master.clone();
    thread::spawn(move || handle_socket_commands(listener, notify_clone, stop_clone, pty_clone));

    match unsafe { forkpty(Some(&PTY_WINSIZE), None) } {
        Ok(ForkptyResult::Parent { child, master }) => {
            parent_process(child, master, notify_pids, ctx, should_stop, pty_master)
        }
        Ok(ForkptyResult::Child) => child_process(direnv_cmd, &ctx.temp_file),
        Err(e) => {
            eprintln!("direnv-instant: forkpty failed: {}", e);
            std::process::exit(1);
        }
    }
}

fn child_process(direnv_cmd: &str, temp_file: &Path) -> ! {
    let mut command = direnv_export_command(direnv_cmd);

    // Set up stdout redirection - if this fails, write error to stderr (PTY)
    let stdout_file = match File::create(temp_file) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("direnv-instant: Failed to create output file: {}", e);
            std::process::exit(1);
        }
    };

    command.stdout(Stdio::from(stdout_file));
    // Let stderr go through PTY (so direnv thinks it's a terminal)

    // Execute direnv - if this fails, write error to stderr (PTY)
    let status = match command.status() {
        Ok(s) => s,
        Err(e) => {
            eprintln!("direnv-instant: Failed to execute {}: {}", direnv_cmd, e);
            std::process::exit(1);
        }
    };

    std::process::exit(status.code().unwrap_or(1));
}

fn copy_pty_to_logfile(
    master: &OwnedFd,
    log_file: &mut File,
    should_stop: &Arc<AtomicBool>,
    ctx: &DaemonContext,
) -> bool {
    use std::time::Instant;

    let mux_delay_ms = mux::mux_delay_ms();

    let mut buf = [0u8; 8192];
    let mut total_bytes = 0;
    let mut mux_spawned = false;
    let start = Instant::now();

    loop {
        if should_stop.load(Ordering::Relaxed) {
            return false;
        }

        let mut fds = FdSet::new();
        fds.insert(master.as_fd());
        let mut timeout = TimeVal::milliseconds(100);

        match select(None, Some(&mut fds), None, None, Some(&mut timeout)) {
            Ok(_) if fds.contains(master.as_fd()) => match read(master, &mut buf) {
                Ok(0) | Err(Errno::EIO) => return true,
                Ok(n) => {
                    total_bytes += n;
                    let _ = log_file.write_all(&buf[..n]);
                    let _ = log_file.flush();
                }
                Err(e) => {
                    eprintln!("direnv-instant: PTY read error: {}", e);
                    return true;
                }
            },
            Err(e) => {
                eprintln!("direnv-instant: PTY select error: {}", e);
                return true;
            }
            _ => {
                // Timeout elapsed, check if we should spawn the multiplexer
                let elapsed_ms = start.elapsed().as_millis() as u64;
                if !mux_spawned
                    && elapsed_ms >= mux_delay_ms
                    && total_bytes > 0
                    && let Some(multiplexer) = ctx.multiplexer
                {
                    let _ = multiplexer.spawn(ctx);
                    mux_spawned = true;
                }
                continue;
            }
        }
    }
}

fn parent_process(
    child: Pid,
    master: OwnedFd,
    notify_pids: Arc<Mutex<Vec<i32>>>,
    ctx: &DaemonContext,
    should_stop: Arc<AtomicBool>,
    pty_master: Arc<Mutex<Option<OwnedFd>>>,
) {
    // Store PTY master fd for WATCH command (duplicate it to keep it alive)
    *pty_master.lock().expect("Failed to lock") = master.try_clone().ok();

    // Create temp stderr file for writing direnv PTY output
    let mut log_file = match File::create(&ctx.temp_stderr) {
        Ok(f) => f,
        Err(e) => {
            eprintln!("direnv-instant: Failed to create stderr log file: {}", e);
            let _ = kill(child, Signal::SIGTERM);
            std::process::exit(1);
        }
    };

    let completed = copy_pty_to_logfile(&master, &mut log_file, &should_stop, &ctx);
    if !completed {
        let _ = kill(child, Signal::SIGTERM);
        return;
    }

    let success = matches!(
        waitpid(child, Some(WaitPidFlag::empty())),
        Ok(WaitStatus::Exited(_, 0))
    );

    // Check if stderr file has actual content (not just empty file we created)
    let has_stderr = ctx
        .temp_stderr
        .metadata()
        .map(|m| m.len() > 0)
        .unwrap_or(false);

    if has_stderr {
        let _ = std::fs::rename(&ctx.temp_stderr, &ctx.stderr_file);
    }
    // Otherwise Cleanup Drop will remove it

    // Only rename env file on success
    let has_env = success && ctx.temp_file.exists();
    if has_env {
        let _ = std::fs::rename(&ctx.temp_file, &ctx.env_file);
    }
    // Otherwise Cleanup Drop will remove it

    // Notify shells if we have anything to show
    if has_stderr || has_env {
        for pid in notify_pids.lock().expect("Failed to lock").iter() {
            let _ = kill(Pid::from_raw(*pid), Signal::SIGUSR1);
        }
    }
}
