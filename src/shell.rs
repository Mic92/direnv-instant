use std::env;
use std::io::Write;

/// Write a line to stdout, ignoring errors. The hook reads our output via
/// `start | source`; if the reader vanishes (ctrl-c, closed pane) the write
/// fails with EPIPE, which is harmless and must not panic (issue #129).
pub fn emit(args: std::fmt::Arguments) {
    let _ = writeln!(std::io::stdout(), "{}", args);
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shell {
    #[default]
    Bash,
    Zsh,
    Fish,
    Nushell,
}

impl Shell {
    pub fn from_env() -> Self {
        match env::var("DIRENV_INSTANT_SHELL").as_deref() {
            Ok("fish") => Shell::Fish,
            Ok("zsh") => Shell::Zsh,
            Ok("nu" | "nushell") => Shell::Nushell,
            _ => Shell::Bash,
        }
    }

    /// Shell name passed to `direnv export`. Nushell has no native format,
    /// so we use json and let the hook parse it via `from json | load-env`.
    pub fn direnv_export_arg(self) -> &'static str {
        match self {
            Shell::Fish => "fish",
            Shell::Nushell => "json",
            Shell::Bash | Shell::Zsh => "zsh",
        }
    }

    /// Print an export statement for this shell.
    /// Nushell is handled separately in [`crate::nushell`]; calls here are unreachable.
    pub fn export_var(self, name: &str, value: &str) {
        let escaped = value.replace('\'', r"'\''");
        match self {
            Shell::Fish => emit(format_args!("set -gx {} '{}'", name, escaped)),
            Shell::Bash | Shell::Zsh => emit(format_args!("export {}='{}'", name, escaped)),
            Shell::Nushell => unreachable!("nushell uses crate::nushell"),
        }
    }

    /// Print an unset statement for this shell
    pub fn unset_var(self, name: &str) {
        match self {
            Shell::Fish => emit(format_args!("set -e {}", name)),
            Shell::Bash | Shell::Zsh => emit(format_args!("unset {}", name)),
            Shell::Nushell => unreachable!("nushell uses crate::nushell"),
        }
    }
}
