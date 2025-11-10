#!/usr/bin/env fish
# direnv-instant.fish - Non-blocking direnv integration for fish
# Provides instant prompts by running direnv asynchronously in the background

# Global state variables
set -g __DIRENV_INSTANT_ENV_FILE ""
set -g __DIRENV_INSTANT_STDERR_FILE ""

# SIGUSR1 handler - loads environment when signaled by Rust daemon
function _direnv_handler --on-signal USR1
    # Display stderr output if available
    if test -n "$__DIRENV_INSTANT_STDERR_FILE" -a -f "$__DIRENV_INSTANT_STDERR_FILE"
        if test -s "$__DIRENV_INSTANT_STDERR_FILE"
            cat "$__DIRENV_INSTANT_STDERR_FILE"
        end
        rm -f "$__DIRENV_INSTANT_STDERR_FILE" 2>/dev/null; or true
        set -g __DIRENV_INSTANT_STDERR_FILE ""
    end

    # Load environment variables (keep file as cache for next time)
    if test -n "$__DIRENV_INSTANT_ENV_FILE" -a -f "$__DIRENV_INSTANT_ENV_FILE"
        source "$__DIRENV_INSTANT_ENV_FILE"
    end
end

# Main hook called on directory changes and prompts
function _direnv_hook --on-variable PWD --on-event fish_prompt
    set -gx DIRENV_INSTANT_SHELL_PID %self

    # Load cached environment immediately if available and caching is enabled
    set -q DIRENV_INSTANT_USE_CACHE; or set -l DIRENV_INSTANT_USE_CACHE 1
    if test "$DIRENV_INSTANT_USE_CACHE" = 1 -a -n "$__DIRENV_INSTANT_ENV_FILE" -a -f "$__DIRENV_INSTANT_ENV_FILE"
        source "$__DIRENV_INSTANT_ENV_FILE"
    end

    direnv-instant start | source
end

# Cleanup on shell exit
function _direnv_exit_cleanup --on-event fish_exit
    direnv-instant stop
end
