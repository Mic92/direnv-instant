# direnv-instant.fish - Non-blocking direnv integration for fish
# Provides instant prompts by running direnv asynchronously in the background

# direnv's classic hook may auto-load from a vendor_conf.d file (e.g. the nix
# package) and would block every prompt alongside us; unregister it.
functions -e __direnv_export_eval __direnv_export_eval_2 __direnv_cd_hook

# Global state variables
set -g __DIRENV_INSTANT_ENV_FILE ""
set -g __DIRENV_INSTANT_STDERR_FILE ""

# SIGUSR1 handler - loads environment when signaled by Rust daemon
function _direnv_handler --on-signal USR1
    # Display stderr output if available
    if test -n "$__DIRENV_INSTANT_STDERR_FILE" -a -f "$__DIRENV_INSTANT_STDERR_FILE"
        if test -s "$__DIRENV_INSTANT_STDERR_FILE"
            command cat "$__DIRENV_INSTANT_STDERR_FILE"
        end
        command rm -f "$__DIRENV_INSTANT_STDERR_FILE" 2>/dev/null; or true
        set -g __DIRENV_INSTANT_STDERR_FILE ""
    end

    # Load environment variables (keep file as cache for next time)
    if test -n "$__DIRENV_INSTANT_ENV_FILE" -a -f "$__DIRENV_INSTANT_ENV_FILE"
        source "$__DIRENV_INSTANT_ENV_FILE"
    end

    # Redraw the prompt; without this the shell looks hung after the replay.
    if status is-interactive
        commandline -f repaint
    end
end

# Main hook called on directory changes and prompts
function _direnv_hook --on-event fish_prompt --on-variable PWD
    set -gx DIRENV_INSTANT_SHELL fish
    set -gx DIRENV_INSTANT_SHELL_PID $fish_pid

    # Load cached environment immediately if available and caching is enabled
    if test "$DIRENV_INSTANT_USE_CACHE" != 0 -a -n "$__DIRENV_INSTANT_ENV_FILE" -a -f "$__DIRENV_INSTANT_ENV_FILE"
        source "$__DIRENV_INSTANT_ENV_FILE"
    end

    direnv-instant start | source
end

# Cleanup on shell exit
function _direnv_exit_cleanup --on-event fish_exit
    direnv-instant stop
end

# Initialize if not already done
if not set -q __DIRENV_INSTANT_HOOKED
    set -g __DIRENV_INSTANT_HOOKED 1

    # Run initial hook
    _direnv_hook
end
