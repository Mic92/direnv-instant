# direnv-instant

Status: Beta

Non-blocking direnv integration daemon with tmux support that provides instant shell prompts by running direnv asynchronously in the background.

## Features

- **Instant Prompts**: No more waiting for direnv to finish loading environments
- **Asynchronous Loading**: Direnv runs in the background, shell gets notified when ready via SIGUSR1
- **Tmux Integration**: Automatically spawns a tmux pane to show direnv output when loading takes too long
- **Shell Support**: Works with both bash and zsh

## How It Works

Instead of blocking your shell prompt while direnv loads environment variables, direnv-instant:

1. Starts a background daemon that runs direnv asynchronously
2. Returns control to your shell immediately for an instant prompt
3. Notifies your shell via SIGUSR1 when the environment is ready
4. Automatically applies the new environment variables without disrupting your workflow
5. If direnv takes longer than 4 seconds (configurable), spawns a tmux pane showing progress

## Installation

### With Nix Flakes

Add to your `flake.nix`:

```nix
{
  inputs.direnv-instant.url = "github:yourusername/direnv-instant";
}
```

Then install:

```bash
nix profile install .#direnv-instant
```

### Building from Source

```bash
cargo build --release
```

## Setup

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

- `DIRENV_INSTANT_TMUX_DELAY`: Delay in seconds before spawning tmux pane (default: 4)
- `DIRENV_INSTANT_DEBUG_LOG`: Path to debug log file for daemon output
- `DIRENV_DIR`: Directory containing `.envrc` (set by direnv)

## Commands

- `direnv-instant start`: Start the daemon for current directory
- `direnv-instant stop`: Stop the daemon for current directory
- `direnv-instant hook <bash|zsh>`: Output shell integration code
- `direnv-instant watch <fifo> <socket>`: Watch direnv output (internal use)

## License

MIT
