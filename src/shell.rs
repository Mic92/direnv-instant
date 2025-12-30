use std::env;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Shell {
    #[default]
    Bash,
    Zsh,
    Fish,
}

impl Shell {
    pub fn from_env() -> Self {
        match env::var("DIRENV_INSTANT_SHELL").as_deref() {
            Ok("fish") => Shell::Fish,
            Ok("zsh") => Shell::Zsh,
            _ => Shell::Bash,
        }
    }

    /// Returns the shell name for direnv export command
    pub fn direnv_export_arg(self) -> &'static str {
        match self {
            Shell::Fish => "fish",
            Shell::Bash | Shell::Zsh => "zsh",
        }
    }

    /// Print an export statement for this shell
    pub fn export_var(self, name: &str, value: &str) {
        let escaped = value.replace('\'', r"'\''");
        match self {
            Shell::Fish => println!("set -gx {} '{}'", name, escaped),
            Shell::Bash | Shell::Zsh => println!("export {}='{}'", name, escaped),
        }
    }

    /// Print an unset statement for this shell
    pub fn unset_var(self, name: &str) {
        match self {
            Shell::Fish => println!("set -e {}", name),
            Shell::Bash | Shell::Zsh => println!("unset {}", name),
        }
    }
}
