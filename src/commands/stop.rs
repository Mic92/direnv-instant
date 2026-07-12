use crate::daemon::{detach_daemon, get_socket_path};
use nix::unistd::getppid;
use std::env;
use std::path::PathBuf;

pub fn run() {
    let shell_pid = env::var("DIRENV_INSTANT_SHELL_PID")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| getppid().as_raw());

    if let Ok(dir) = env::var("__DIRENV_INSTANT_CURRENT_DIR") {
        let socket_path = get_socket_path(&PathBuf::from(dir));
        detach_daemon(&socket_path, shell_pid);
    }
}
