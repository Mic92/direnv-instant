//! Regression test for #105: `select()` returning EINTR (e.g. on SIGWINCH)
//! must not kill the watch loop.

mod common;
use common::*;
use nix::pty::openpty;
use nix::sys::signal::{Signal, kill};
use nix::sys::socket::{ControlMessage, MsgFlags, sendmsg};
use nix::unistd::Pid;
use std::io::IoSlice;
use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::os::unix::net::{UnixListener, UnixStream};
use std::process::{Command, Stdio};
use std::time::Duration;

#[test]
fn watch_survives_sigwinch() {
    let sb = Sandbox::new("true\n").unwrap();
    let log_path = sb.dir.join("watch.log");
    std::fs::write(&log_path, "").unwrap();
    let socket_path = sb.dir.join("daemon.sock");

    // The watch command requests the PTY fd from the daemon socket.
    // Hand ownership of the slave fd to Stdio so it is closed exactly once.
    let pty = openpty(None, None).unwrap();
    let master: OwnedFd = pty.master;
    let slave: OwnedFd = pty.slave;
    let listener = UnixListener::bind(&socket_path).unwrap();

    let mut watch = Command::new(bin())
        .args([
            "watch",
            log_path.to_str().unwrap(),
            socket_path.to_str().unwrap(),
        ])
        .stdin(Stdio::from(slave))
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();

    // Reply to the WATCH request with the PTY master over SCM_RIGHTS.
    let (watch_conn, _) = listener.accept().unwrap();
    let mut buf = [0u8; 64];
    use std::io::Read;
    let n = (&watch_conn).read(&mut buf).unwrap();
    assert!(buf[..n].starts_with(b"WATCH"), "unexpected request");
    let fds = [master.as_raw_fd()];
    let cmsg = [ControlMessage::ScmRights(&fds)];
    sendmsg::<()>(
        watch_conn.as_fd().as_raw_fd(),
        &[IoSlice::new(b"OK\n")],
        &cmsg,
        MsgFlags::empty(),
        None,
    )
    .unwrap();
    drop(watch_conn);

    // Long-lived monitoring connection; watch exits when it closes.
    let monitoring: UnixStream = listener.accept().unwrap().0;

    std::thread::sleep(Duration::from_millis(500));
    assert!(watch.try_wait().unwrap().is_none(), "watch exited early");

    kill(Pid::from_raw(watch.id() as i32), Signal::SIGWINCH).unwrap();
    std::thread::sleep(Duration::from_millis(500));
    assert!(
        watch.try_wait().unwrap().is_none(),
        "watch exited after SIGWINCH"
    );

    drop(monitoring);
    drop(listener);
    let _ = watch.wait();
}
