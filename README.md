# direnv-instant

Non-blocking direnv integration daemon with tmux/zellij support that provides instant shell prompts by running direnv asynchronously in the background.

## Features

- **Instant Prompts**: No more waiting for direnv to finish loading environments
- **Environment Caching**: Uses cached environment from previous load for truly instant prompts
- **Asynchronous Loading**: Direnv runs in the background, shell gets notified when ready via SIGUSR1
- **Multiplexer Integration**: Automatically spawns a tmux/zellij pane to show direnv output when loading takes too long
- **Shell Support**: Works with both bash and zsh

## How It Works

Instead of blocking your shell prompt while direnv loads environment variables, direnv-instant:

1. Starts a background daemon that runs direnv asynchronously
2. Returns control to your shell immediately for an instant prompt
3. Notifies your shell via SIGUSR1 when the environment is ready
4. Automatically applies the new environment variables without disrupting your workflow
5. If direnv takes longer than 4 seconds (configurable), spawns a tmux/zellij pane showing progress

## Supported multiplexers
- Kitty (with `-o allow_remote_control=yes --listen-on unix:"$(mktemp)"` only)
- Tmux
- Wezterm
- Zellij

## Recommended

For Nix users, we highly recommend using [nix-direnv](https://github.com/nix-community/nix-direnv) alongside direnv-instant. It provides intelligent caching of Nix environments and creates gcroots to prevent garbage collection, which is essential for direnv-instant's environment caching to work reliably.

## Installation

### Home Manager

Add to your `flake.nix` inputs:

```nix
{
  inputs.direnv-instant.url = "github:Mic92/direnv-instant";
}
```

Then make `inputs` available to your home-manager modules via `extraSpecialArgs`:

```nix
homeConfigurations."user" = home-manager.lib.homeManagerConfiguration {
  # ... other config ...
  extraSpecialArgs = { inherit inputs; };
  modules = [
    ./home.nix
  ];
};
```

Now add to your home-manager configuration:

```nix
{ inputs, pkgs, ... }:
{
  home.packages = [
    inputs.direnv-instant.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

### NixOS

Add to your `flake.nix` inputs:

```nix
{
  inputs.direnv-instant.url = "github:Mic92/direnv-instant";
}
```

Then make `inputs` available to your NixOS modules by adding `specialArgs`:

```nix
nixosSystem {
  # ... other config ...
  specialArgs = { inherit inputs; };
  modules = [
    ./configuration.nix
  ];
}
```

Now add to your NixOS configuration:

```nix
{ inputs, pkgs, ... }:
{
  environment.systemPackages = [
    inputs.direnv-instant.packages.${pkgs.stdenv.hostPlatform.system}.default
  ];
}
```

### Adhoc testing

For zsh:
```bash
eval "$(nix run github:Mic92/direnv-instant -- hook zsh)"
```

For bash:
```bash
eval "$(nix run github:Mic92/direnv-instant -- hook bash)"
```

### Building from Source

```bash
cargo build --release
```

## Setup

**IMPORTANT:** direnv-instant replaces direnv's normal shell integration. Do NOT use both together. Remove any existing `eval "$(direnv hook bash)"` or `eval "$(direnv hook zsh)"` from your shell configuration before adding direnv-instant.

### Bash

Add to your `~/.bashrc`:

```bash
eval "$(direnv-instant hook bash)"
```

### Zsh

Add to your `~/.zshrc`:

```bash
eval "$(direnv-instant hook zsh)"
```

## Configuration

### Environment Variables

- `DIRENV_INSTANT_USE_CACHE`: Enable cached environment loading for instant prompts (default: 1). Set to 0 to disable caching.
- `DIRENV_INSTANT_MUX_DELAY`: Delay in seconds before spawning multiplexer pane (default: 4)
- `DIRENV_INSTANT_DEBUG_LOG`: Path to debug log file for daemon output

## FAQ

### How does direnv-instant differ from lorri?

While both tools provide automatic environment rebuilding for Nix projects, direnv-instant offers several key usability improvements:

- **Built-in visibility**: After 4 seconds (configurable), direnv-instant automatically spawns a tmux/zellij split pane showing direnv output. You don't need to separately monitor journal logs to see what's happening.
- **Transparent rebuilds**: With lorri, you have to actively watch the journal to know if it's doing heavy rebuilds. direnv-instant makes this visible by default in your terminal.
- **Interruptible**: Unlike lorri, you can ctrl-c to interrupt operations when needed.
- **Shell integration focused**: direnv-instant is specifically designed as a drop-in replacement for direnv's shell integration, working with any direnv-compatible project.

## License

MIT
