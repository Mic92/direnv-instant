use std::{
    env,
    io::{self, Error},
    process::Command,
};

use crate::daemon::DaemonContext;

const PANE_HEIGHT: &str = "10";
const KITTY_VAR: &str = "KITTY_LISTEN_ON";

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Multiplexer {
    Tmux,
    Zellij,
    Wezterm,
    Kitty,
}

impl Multiplexer {
    pub fn detect() -> Option<Self> {
        if env::var("TMUX").is_ok() {
            return Some(Self::Tmux);
        }

        if env::var("ZELLIJ").is_ok() {
            return Some(Self::Zellij);
        }

        if env::var("TERM_PROGRAM").is_ok_and(|x| x == "WezTerm") {
            return Some(Self::Wezterm);
        }

        if env::var(KITTY_VAR).is_ok() {
            return Some(Self::Kitty);
        }

        None
    }

    pub fn spawn(&self, ctx: &DaemonContext) -> io::Result<()> {
        // Use full path to binary so the multiplexer can find it
        let bin = env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "direnv-instant".to_string());

        let mux_bin = match self {
            Multiplexer::Tmux => "tmux",
            Multiplexer::Zellij => "zellij",
            Multiplexer::Wezterm => "wezterm",
            Multiplexer::Kitty => "kitty",
        };

        let mux_args = match self {
            Multiplexer::Tmux => vec!["split-window", "-d", "-l", PANE_HEIGHT],
            Multiplexer::Zellij => vec![
                "action",
                "new-pane",
                "-d",
                "down",
                "--width",
                PANE_HEIGHT,
                "--",
            ],
            Multiplexer::Wezterm => vec!["cli", "split-pane", "--bottom", "--cells", PANE_HEIGHT],
            Multiplexer::Kitty => vec!["launch", "--location", "vsplit", "--keep-focus", "--self"],
        };

        let mut command = Command::new(mux_bin);

        if *self == Multiplexer::Kitty {
            let kitty_listen_on = env::var(KITTY_VAR).map_err(|e| Error::other(e.to_string()))?;
            command.args(["@", "--to", kitty_listen_on.as_str()]);
        }

        command
            .args(mux_args)
            .args([
                &bin,
                "watch",
                &ctx.temp_stderr.to_string_lossy(),
                &ctx.socket_path.to_string_lossy(),
            ])
            .spawn()
            .map(|_| ())
    }
}

pub fn mux_delay_ms() -> u64 {
    env::var("DIRENV_INSTANT_MUX_DELAY")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| s * 1000)
        .unwrap_or(4000)
}
