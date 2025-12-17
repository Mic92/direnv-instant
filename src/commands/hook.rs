pub fn run(shell: &str) {
    match shell {
        "zsh" => print!("{}", include_str!("../../hooks/zsh.sh")),
        "bash" => print!("{}", include_str!("../../hooks/bash.sh")),
        "fish" => print!("{}", include_str!("../../hooks/fish.fish")),
        _ => {
            eprintln!("Unsupported shell: {}", shell);
            std::process::exit(1);
        }
    }
}
