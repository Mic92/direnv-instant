use std::env;

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
            Shell::Fish => println!("set -gx {} '{}'", name, escaped),
            Shell::Bash | Shell::Zsh => println!("export {}='{}'", name, escaped),
            Shell::Nushell => unreachable!("nushell uses crate::nushell"),
        }
    }

    /// Print an unset statement for this shell
    pub fn unset_var(self, name: &str) {
        match self {
            Shell::Fish => println!("set -e {}", name),
            Shell::Bash | Shell::Zsh => println!("unset {}", name),
            Shell::Nushell => unreachable!("nushell uses crate::nushell"),
        }
    }
}
