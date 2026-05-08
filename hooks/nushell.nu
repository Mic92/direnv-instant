# direnv-instant hook for Nushell.
#
# Nushell has no signal-trap mechanism, so unlike the bash/zsh/fish hooks
# this one cannot react to SIGUSR1 from the daemon. Instead it polls the
# env file at pre_prompt and pre_execution. The binary speaks JSON because
# `source` in nu is parse-time and cannot evaluate dynamic command output.
#
# All env mutations happen in a single `--env` def call to dodge a nushell
# quirk where hide-env after load-env across separate `--env` defs in the
# same call chain does not propagate.

def _direnv_instant_load_file [path: string]: nothing -> record {
    if ($path | is-empty) or not ($path | path exists) { return {} }
    open --raw $path | from json | default {}
}

def --env _direnv_instant_apply [data: record] {
    let to_load = (
        $data | items {|k v| {key: $k, val: $v} } | where val != null
        | reduce -f {} {|it acc| $acc | upsert $it.key $it.val }
    )
    let to_unset = ($data | items {|k v| if $v == null { $k } } | compact)
    for k in $to_unset { hide-env --ignore-errors $k }
    if not ($to_load | is-empty) { load-env $to_load }
}

def --env _direnv_instant_show_stderr [] {
    let f = ($env.__DIRENV_INSTANT_STDERR_FILE? | default "")
    if ($f | is-empty) or not ($f | path exists) { return }
    let content = (open --raw $f)
    if not ($content | is-empty) { print --stderr $content }
    rm --force $f
}

def --env _direnv_instant_hook [] {
    $env.DIRENV_INSTANT_SHELL = "nu"
    $env.DIRENV_INSTANT_SHELL_PID = ($nu.pid | into string)
    let cached = (_direnv_instant_load_file ($env.__DIRENV_INSTANT_ENV_FILE? | default ""))
    let raw = (^direnv-instant start | str trim)
    let state = if ($raw | is-empty) { {} } else { $raw | from json }
    let next_file = ($state.__DIRENV_INSTANT_ENV_FILE? | default ($env.__DIRENV_INSTANT_ENV_FILE? | default ""))
    let fresh = (_direnv_instant_load_file $next_file)
    _direnv_instant_apply ($cached | merge $state | merge $fresh)
    _direnv_instant_show_stderr
}

export-env {
    $env.config = ($env.config? | default {})
    $env.config.hooks = ($env.config.hooks? | default {})
    $env.config.hooks.pre_prompt = (
        ($env.config.hooks.pre_prompt? | default []) | append { _direnv_instant_hook }
    )
    $env.config.hooks.pre_execution = (
        ($env.config.hooks.pre_execution? | default []) | append { _direnv_instant_hook }
    )
}
