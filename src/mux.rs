use std::{
    collections::HashMap,
    env,
    io::{self, Error},
    path::PathBuf,
    process::Command,
};

use tinyjson::JsonValue;

use crate::daemon::DaemonContext;

const PANE_HEIGHT: &str = "10";
const KITTY_VAR: &str = "KITTY_LISTEN_ON";
const KITTY_LAUNCH_ARGS_VAR: &str = "DIRENV_INSTANT_KITTY_LAUNCH_ARGS";
const DEFAULT_KITTY_LAUNCH_ARGS: [&str; 4] = ["--location", "vsplit", "--keep-focus", "--self"];

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Multiplexer {
    Tmux,
    Zellij,
    Wezterm,
    Kitty,
    Herdr,
    Rex,
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

        if env::var("HERDR_ENV").is_ok_and(|x| x == "1") {
            return Some(Self::Herdr);
        }

        if env::var("TERM_PROGRAM").is_ok_and(|x| x == "rex") && env::var("REX_BLOCK").is_ok() {
            return Some(Self::Rex);
        }

        None
    }

    pub fn spawn(&self, ctx: &DaemonContext) -> io::Result<()> {
        // Use full path to binary so the multiplexer can find it
        let bin = env::current_exe()
            .ok()
            .and_then(|p| p.to_str().map(String::from))
            .unwrap_or_else(|| "direnv-instant".to_string());

        let mut command;

        match self {
            Multiplexer::Herdr => return spawn_herdr(&bin, ctx),
            Multiplexer::Rex => return spawn_rex(&bin, ctx),
            Multiplexer::Tmux => {
                command = Command::new("tmux");
                command.args(["split-window", "-d", "-l", PANE_HEIGHT]);
            }
            Multiplexer::Zellij => {
                command = Command::new("zellij");
                command.args([
                    "action",
                    "new-pane",
                    "-d",
                    "down",
                    "--width",
                    PANE_HEIGHT,
                    "--close-on-exit",
                    "--",
                ]);
            }
            Multiplexer::Wezterm => {
                command = Command::new("wezterm");
                command.args(["cli", "split-pane", "--bottom", "--cells", PANE_HEIGHT]);
            }
            Multiplexer::Kitty => {
                let kitty_listen_on =
                    env::var(KITTY_VAR).map_err(|e| Error::other(e.to_string()))?;
                command = Command::new("kitty");
                command.args(["@", "--to", kitty_listen_on.as_str()]);
                command.arg("launch").args(kitty_launch_args());
            }
        }

        command
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

/// Herdr needs two steps: split a shell pane, then exec the watcher in it.
fn spawn_herdr(bin: &str, ctx: &DaemonContext) -> io::Result<()> {
    let ratio = format!("{:.3}", herdr_split_ratio().unwrap_or(0.75));
    let output = Command::new("herdr")
        .args([
            "pane",
            "split",
            "--current",
            "--direction",
            "down",
            "--no-focus",
            "--ratio",
            &ratio,
        ])
        .output()?;
    if !output.status.success() {
        return Err(Error::other(format!(
            "herdr pane split failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let pane_id = parse_pane_id(&stdout)
        .ok_or_else(|| Error::other("herdr pane split: no pane_id in response"))?;

    let watch_cmd = format!(
        "exec {} watch {} {}",
        shell_quote(bin),
        shell_quote(&ctx.temp_stderr.to_string_lossy()),
        shell_quote(&ctx.socket_path.to_string_lossy()),
    );

    Command::new("herdr")
        .args(["pane", "run", &pane_id, &watch_cmd])
        .spawn()
        .map(|_| ())
}

/// Rex needs two steps too: split below the shell's block, then size the split.
/// The app resets `rex split --ratio` to half when it shows the split, so a resize sizes it afterwards.
fn spawn_rex(bin: &str, ctx: &DaemonContext) -> io::Result<()> {
    let block = env::var("REX_BLOCK").map_err(|e| Error::other(e.to_string()))?;
    let ratio = rex_split_ratio(&block).unwrap_or(0.75);
    let temp_stderr = ctx.temp_stderr.to_string_lossy();
    let socket_path = ctx.socket_path.to_string_lossy();
    let output = rex_command()
        .args([
            "split",
            "--json",
            "--block",
            &block,
            "--split=below",
            "--focus=false",
            "--shell=none",
            "--",
            bin,
            "watch",
            &temp_stderr,
            &socket_path,
        ])
        .output()?;
    if !output.status.success() {
        return Err(Error::other(format!(
            "rex split failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }

    let watcher = parse_rex_block_id(&String::from_utf8_lossy(&output.stdout))
        .ok_or_else(|| Error::other("rex split: no block id in output"))?;

    // Only the resize needs the session, so read it after the split, which
    // works without it and still gives a pane when it is missing.
    let session = env::var("REX_SESSION").map_err(|e| Error::other(e.to_string()))?;
    let args = JsonValue::from(HashMap::from([
        ("session_id".to_string(), JsonValue::from(session)),
        ("before_block_id".to_string(), JsonValue::from(block)),
        ("after_block_id".to_string(), JsonValue::from(watcher)),
        (
            "direction".to_string(),
            JsonValue::from("vertical".to_string()),
        ),
        ("ratio".to_string(), JsonValue::from(ratio)),
    ]))
    .stringify()
    .map_err(|e| Error::other(e.to_string()))?;
    let output = rex_command()
        .args(["api", "call", "session.resize_pane", &args])
        .output()?;
    if !output.status.success() {
        return Err(Error::other(format!(
            "rex resize failed: {}",
            String::from_utf8_lossy(&output.stderr)
        )));
    }
    Ok(())
}

/// Rex exports `REX_BIN_DIR`, so its CLI is found even when it is not on PATH.
/// `--autostart=false` keeps the detached daemon from starting a server after Rex's has stopped.
fn rex_command() -> Command {
    let rex = env::var_os("REX_BIN_DIR")
        .map(|dir| PathBuf::from(dir).join("rex"))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| PathBuf::from("rex"));
    let mut command = Command::new(rex);
    command.arg("--autostart=false");
    command
}

/// Rex's split ratio is the fraction the shell's block keeps:
/// leave ~PANE_HEIGHT rows for the new watcher pane.
fn rex_split_ratio(block: &str) -> Option<f64> {
    let output = rex_command()
        .args([
            "do",
            "--block",
            block,
            "com.superlogical.terminal.size",
            "--json",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: JsonValue = String::from_utf8_lossy(&output.stdout).parse().ok()?;
    split_ratio(*json_get(&value, &["rows"])?.get()?)
}

/// Herdr's split ratio is the fraction the original pane keeps:
/// leave ~PANE_HEIGHT rows for the new watcher pane.
fn herdr_split_ratio() -> Option<f64> {
    let pane_id = env::var("HERDR_PANE_ID").ok()?;
    let output = Command::new("herdr")
        .args(["pane", "layout", "--current"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let json = String::from_utf8_lossy(&output.stdout);
    split_ratio(parse_pane_height(&json, &pane_id)?)
}

/// The fraction a pane of `rows` rows keeps when ~PANE_HEIGHT rows go to the watcher pane.
fn split_ratio(rows: f64) -> Option<f64> {
    if rows <= 0.0 {
        return None;
    }
    let target: f64 = PANE_HEIGHT.parse().ok()?;
    Some((1.0 - target / rows).clamp(0.5, 0.9))
}

/// Safe nested object lookup: tinyjson's `Index` panics on missing keys.
fn json_get<'a>(value: &'a JsonValue, path: &[&str]) -> Option<&'a JsonValue> {
    path.iter().try_fold(value, |v, key| {
        v.get::<HashMap<String, JsonValue>>()?.get(*key)
    })
}

fn parse_pane_height(json: &str, pane_id: &str) -> Option<f64> {
    let value: JsonValue = json.parse().ok()?;
    let panes: &Vec<JsonValue> = json_get(&value, &["result", "layout", "panes"])?.get()?;
    let pane = panes.iter().find(|p| {
        json_get(p, &["pane_id"])
            .and_then(JsonValue::get::<String>)
            .is_some_and(|id| id == pane_id)
    })?;
    json_get(pane, &["rect", "height"])?.get().copied()
}

fn parse_pane_id(json: &str) -> Option<String> {
    let value: JsonValue = json.parse().ok()?;
    json_get(&value, &["result", "pane", "pane_id"])?
        .get::<String>()
        .cloned()
}

fn parse_rex_block_id(json: &str) -> Option<String> {
    let value: JsonValue = json.parse().ok()?;
    let ids: &Vec<JsonValue> = json_get(&value, &["block_ids"])?.get()?;
    ids.first()?.get::<String>().cloned()
}

fn shell_quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', "'\\''"))
}

fn kitty_launch_args() -> Vec<String> {
    match env::var(KITTY_LAUNCH_ARGS_VAR) {
        Ok(args) => args
            .lines()
            .filter(|arg| !arg.is_empty())
            .map(String::from)
            .collect(),
        Err(_) => DEFAULT_KITTY_LAUNCH_ARGS
            .iter()
            .map(|arg| (*arg).to_string())
            .collect(),
    }
}

pub fn mux_delay_ms() -> u64 {
    env::var("DIRENV_INSTANT_MUX_DELAY")
        .ok()
        .and_then(|s| s.parse::<u64>().ok())
        .map(|s| s * 1000)
        .unwrap_or(4000)
}
