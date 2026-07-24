# direnv-instant

Non-blocking direnv shell integration. Runs direnv in a background daemon so
your prompt comes back immediately instead of waiting for `.envrc` to finish.

![Demo](https://github.com/Mic92/direnv-instant/releases/download/assets/demo.gif)

## What it does

The shell hook starts a daemon and returns right away. The daemon runs direnv,
writes the resulting environment to a file, and signals the shell with SIGUSR1
when it's done. The shell trap then loads the new environment. On revisit, a
cached environment from the previous run is applied instantly while the daemon
revalidates in the background.

If direnv takes longer than 4 seconds (configurable), a tmux/zellij/wezterm/kitty/herdr
split opens showing direnv's output so you can see what it's doing — and ctrl-c
it if needed.

Supported shells: bash, zsh, fish, nushell.
Supported multiplexers: tmux, zellij, wezterm, kitty (kitty needs the
home-manager module), herdr.

If you use Nix, pair this with
[nix-direnv](https://github.com/nix-community/nix-direnv). It caches Nix
environments and creates gcroots, which keeps direnv-instant's cached
environment from being garbage collected out from under you.

## Installation

direnv-instant replaces direnv's normal shell integration — don't use both.
Remove any `eval "$(direnv hook ...)"` lines from your shell config first.

### Home Manager

```nix
{
  inputs.direnv-instant.url = "github:Mic92/direnv-instant";
}
```

Pass `inputs` through `extraSpecialArgs`, then:

```nix
{ inputs, ... }:
{
  imports = [ inputs.direnv-instant.homeModules.direnv-instant ];
  programs.direnv-instant.enable = true;
}
```

### NixOS

Same flake input. Pass `inputs` through `specialArgs`, then:

```nix
{ inputs, ... }:
{
  imports = [ inputs.direnv-instant.nixosModules.direnv-instant ];
  programs.direnv-instant.enable = true;
}
```

### Manual shell setup

bash (`~/.bashrc`):
```bash
eval "$(direnv-instant hook bash)"
```

zsh (`~/.zshrc`):
```bash
eval "$(direnv-instant hook zsh)"
```

fish (`~/.config/fish/config.fish`):
```fish
direnv-instant hook fish | source
```

fish note: the nix direnv package ships `share/fish/vendor_conf.d/direnv.fish`,
which fish auto-loads even though no hook line appears in your config. The
direnv-instant hook unregisters those handlers, so the `config.fish` line above
is enough.

nushell: there's no `hook nu | source` one-liner because nushell's `source` is
parse-time only. The hook ships at `share/direnv-instant/nushell.nu`; the
home-manager module sources it for you when
`programs.direnv-instant.enableNushellIntegration` is enabled. Nushell also
can't trap SIGUSR1, so the hook polls the env file on each prompt — new env
shows up on the next prompt after direnv finishes.

For a quick test without installing, swap `direnv-instant` for
`nix run github:Mic92/direnv-instant --` in the lines above.

### From source

```bash
cargo build --release
```

## Configuration

Module options under `programs.direnv-instant`:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enable` | bool | `false` | Enable direnv-instant |
| `package` | package | built-in | Package to use |
| `enableBashIntegration` | bool | `true` | Install bash hook |
| `enableZshIntegration` | bool | `true` | Install zsh hook |
| `enableFishIntegration` | bool | `true` | Install fish hook |
| `settings.use_cache` | bool | `true` | Apply cached env instantly on prompt |
| `settings.mux_delay` | int | `4` | Seconds before opening a multiplexer pane |
| `settings.kitty_launch_args` | listOf str | `["--location" "vsplit" "--keep-focus" "--self"]` | Args passed to `kitty launch` |
| `settings.debug_log` | string | `null` | Path to daemon debug log |

Home Manager only:

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `enableNushellIntegration` | bool | `true` | Install nushell hook |
| `enableKittyIntegration` | bool | `config.programs.kitty.enable` | Configure kitty remote control |

Outside the modules, the same settings are read from environment variables:
`DIRENV_INSTANT_USE_CACHE`, `DIRENV_INSTANT_MUX_DELAY`,
`DIRENV_INSTANT_KITTY_LAUNCH_ARGS` (newline-separated), `DIRENV_INSTANT_DEBUG_LOG`.

## How is this different from lorri?

Both run nix evaluation in the background. direnv-instant shows you what's
happening: after a few seconds it opens a multiplexer pane with direnv's output
instead of making you tail journal logs, and you can ctrl-c a stuck rebuild. It
also works with any direnv `.envrc`, not just Nix shells.

## License

MIT
