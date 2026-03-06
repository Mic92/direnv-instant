use crate::daemon::stop_daemon;
use nix::libc;
use nix::pty::Winsize;
use nix::sys::select::{FdSet, select};
use nix::sys::signal::{SaFlags, SigAction, SigHandler, SigSet, Signal, sigaction};
use nix::sys::socket::{ControlMessageOwned, MsgFlags, recvmsg};
use nix::sys::termios::{InputFlags, LocalFlags, SetArg, Termios, tcgetattr, tcsetattr};
use nix::sys::time::{TimeVal, TimeValLike};
use nix::unistd::{isatty, read, write};
use std::fs::File;
use std::io::{self, IoSliceMut, Stdin, Write};
use std::os::fd::{AsFd, AsRawFd, FromRawFd, OwnedFd, RawFd};
use std::os::unix::net::UnixStream;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

static WATCH_RUNNING: AtomicBool = AtomicBool::new(true);
static SIGWINCH_RECEIVED: AtomicBool = AtomicBool::new(false);

extern "C" fn sigint_handler(_: nix::libc::c_int) {
    WATCH_RUNNING.store(false, Ordering::SeqCst);
}

extern "C" fn sigwinch_handler(_: nix::libc::c_int) {
    SIGWINCH_RECEIVED.store(true, Ordering::SeqCst);
}

/// Read the terminal window size from a file descriptor.
fn get_winsize(fd: RawFd) -> Option<Winsize> {
    let mut ws: Winsize = unsafe { std::mem::zeroed() };
    let ret = unsafe { libc::ioctl(fd, libc::TIOCGWINSZ, &mut ws) };
    if ret == 0 { Some(ws) } else { None }
}

/// Set the terminal window size on a file descriptor.
/// When called on a PTY master, the kernel delivers SIGWINCH to the
/// child process group, causing programs like nix to re-read the size.
fn set_winsize(fd: RawFd, ws: &Winsize) {
    unsafe {
        libc::ioctl(fd, libc::TIOCSWINSZ, ws);
    }
}

/// Propagate our terminal size to the PTY master so the child process
/// (e.g. nix) uses the actual pane width instead of a hardcoded default.
fn propagate_winsize(pty_fd: RawFd) {
    if let Some(ws) = get_winsize(libc::STDERR_FILENO) {
        set_winsize(pty_fd, &ws);
    }
}

/// RAII guard that restores terminal settings on drop
struct RawModeGuard<'a> {
    stdin: &'a Stdin,
    original_termios: Termios,
}

impl<'a> RawModeGuard<'a> {
    fn new(stdin: &'a Stdin) -> Option<Self> {
        let original_termios = tcgetattr(stdin).ok()?;
        let mut raw_termios = original_termios.clone();

        // Disable canonical mode and echo
        raw_termios
            .local_flags
            .remove(LocalFlags::ICANON | LocalFlags::ECHO | LocalFlags::ISIG);
        // Disable input processing (Ctrl-C, Ctrl-Z, etc.)
        raw_termios
            .input_flags
            .remove(InputFlags::ICRNL | InputFlags::IXON);

        tcsetattr(stdin, SetArg::TCSANOW, &raw_termios).ok()?;

        Some(RawModeGuard {
            stdin,
            original_termios,
        })
    }
}

impl Drop for RawModeGuard<'_> {
    fn drop(&mut self) {
        let _ = tcsetattr(self.stdin, SetArg::TCSANOW, &self.original_termios);
    }
}

pub fn run(log_path: &Path, socket_path: &Path) {
    let handler = SigHandler::Handler(sigint_handler);
    let action = SigAction::new(handler, SaFlags::empty(), SigSet::empty());
    unsafe {
        let _ = sigaction(Signal::SIGINT, &action);
    }

    // Open log file for reading (should exist by now, but wait up to 5 seconds as safety margin)
    let log_file = {
        let start = Instant::now();
        let timeout = Duration::from_secs(5);
        loop {
            if let Ok(f) = File::open(log_path) {
                break f;
            }
            if start.elapsed() > timeout {
                eprintln!("Timeout waiting for log file");
                std::process::exit(1);
            }
            std::thread::sleep(Duration::from_millis(100));
        }
    };

    let stdin = io::stdin();
    let stdin_is_terminal = isatty(&stdin).unwrap_or(false);

    // Socket 1: Get PTY fd if stdin is a terminal (then close)
    let pty_master = if stdin_is_terminal {
        match UnixStream::connect(socket_path) {
            Ok(mut watch_socket) => {
                // Request PTY fd
                if watch_socket.write_all(b"WATCH\n").is_err() {
                    None
                } else {
                    // Wait for response with timeout (5 seconds)
                    let mut fds = FdSet::new();
                    fds.insert(watch_socket.as_fd());
                    let mut timeout = TimeVal::seconds(5);

                    match select(None, Some(&mut fds), None, None, Some(&mut timeout)) {
                        Ok(_) if fds.contains(watch_socket.as_fd()) => {
                            // Receive PTY master fd via SCM_RIGHTS
                            let mut iov = [0u8; 16];
                            let mut iov_slice = [IoSliceMut::new(&mut iov)];
                            let mut cmsg_space = nix::cmsg_space!([RawFd; 1]);

                            match recvmsg::<()>(
                                watch_socket.as_raw_fd(),
                                &mut iov_slice,
                                Some(&mut cmsg_space),
                                MsgFlags::empty(),
                            ) {
                                Ok(msg) => {
                                    let mut received_fd = None;
                                    if let Ok(cmsgs) = msg.cmsgs() {
                                        for cmsg in cmsgs {
                                            if let ControlMessageOwned::ScmRights(fds) = cmsg
                                                && let Some(&fd) = fds.first()
                                            {
                                                received_fd =
                                                    Some(unsafe { OwnedFd::from_raw_fd(fd) });
                                                break;
                                            }
                                        }
                                    }
                                    received_fd
                                }
                                Err(e) => {
                                    eprintln!("direnv-instant: Failed to receive PTY fd: {}", e);
                                    None
                                }
                            }
                        }
                        Ok(_) => {
                            // Timeout: daemon didn't respond in time
                            eprintln!("direnv-instant: Timeout waiting for PTY fd from daemon");
                            None
                        }
                        Err(e) => {
                            eprintln!(
                                "direnv-instant: select() error while waiting for PTY fd: {}",
                                e
                            );
                            None
                        }
                    }
                }
                // watch_socket is dropped here, closing the connection
            }
            Err(_) => None,
        }
    } else {
        None
    };

    // Propagate our terminal size to the PTY and track future resizes
    if let Some(ref pty) = pty_master {
        propagate_winsize(pty.as_raw_fd());

        let winch_handler = SigHandler::Handler(sigwinch_handler);
        let winch_action = SigAction::new(winch_handler, SaFlags::empty(), SigSet::empty());
        unsafe {
            let _ = sigaction(Signal::SIGWINCH, &winch_action);
        }
    }

    // Set stdin to raw mode for transparent input forwarding (only if we have PTY fd)
    let _raw_mode_guard = if pty_master.is_some() {
        RawModeGuard::new(&stdin)
    } else {
        None
    };

    // Socket 2: Monitor daemon exit (long-lived connection)
    let socket = UnixStream::connect(socket_path).unwrap_or_else(|_| std::process::exit(1));

    let mut buf = [0u8; 8192];
    let stdout = io::stdout();
    let mut handle = stdout.lock();

    while WATCH_RUNNING.load(Ordering::SeqCst) {
        // Propagate terminal resize to PTY when our pane is resized
        if SIGWINCH_RECEIVED.swap(false, Ordering::SeqCst)
            && let Some(ref pty) = pty_master
        {
            propagate_winsize(pty.as_raw_fd());
        }

        let mut fds = FdSet::new();
        fds.insert(socket.as_fd());
        if stdin_is_terminal && pty_master.is_some() {
            fds.insert(stdin.as_fd());
        }
        let mut timeout = TimeVal::milliseconds(100);

        match select(None, Some(&mut fds), None, None, Some(&mut timeout)) {
            Ok(_) => {
                // Check if stdin has data to forward to PTY
                if stdin_is_terminal
                    && fds.contains(stdin.as_fd())
                    && let Some(ref pty) = pty_master
                {
                    match read(&stdin, &mut buf) {
                        Ok(0) => {
                            // EOF on stdin
                            break;
                        }
                        Ok(n) => {
                            // Forward input to PTY master
                            let _ = write(pty, &buf[..n]);
                        }
                        Err(_) => {}
                    }
                }

                // Check if socket closed (daemon done)
                if fds.contains(socket.as_fd()) {
                    match read(&socket, &mut buf) {
                        Ok(0) => {
                            // Socket closed - daemon is done, output remaining log data and exit
                            while let Ok(n) = read(&log_file, &mut buf) {
                                if n == 0 {
                                    break;
                                }
                                let _ = handle.write_all(&buf[..n]);
                            }
                            let _ = handle.flush();
                            break;
                        }
                        Ok(_) => {
                            // Unexpected data on socket, ignore
                        }
                        Err(_) => break,
                    }
                }
            }
            Err(_) => break,
        }

        // Poll log file on every iteration (regular files don't work with select)
        match read(&log_file, &mut buf) {
            Ok(0) => {
                // No data available yet, continue
            }
            Ok(n) => {
                let _ = handle.write_all(&buf[..n]);
                let _ = handle.flush();
            }
            Err(_) => {}
        }
    }

    if !WATCH_RUNNING.load(Ordering::SeqCst) {
        stop_daemon(socket_path);
    }
}
