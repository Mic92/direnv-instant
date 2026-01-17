#!/usr/bin/env zsh
# shellcheck disable=SC1071
# direnv-instant.zsh - Non-blocking direnv integration for zsh
# Provides instant prompts by running direnv asynchronously in the background

# Global state variables
typeset -g __DIRENV_INSTANT_ENV_FILE=""
typeset -g __DIRENV_INSTANT_STDERR_FILE=""

# Save existing TRAPUSR1 handler if another plugin defined one
if (( $+functions[TRAPUSR1] )); then
  functions[__direnv_instant_orig_TRAPUSR1]="$functions[TRAPUSR1]"
fi

# SIGUSR1 handler - loads environment when signaled by Rust daemon
# Use TRAPUSR1 function instead of 'trap ... USR1' because:
# - Function traps are not reset in subshells (zsh behavior)
# - Provides proper function context for debugging
# - Inspectable via 'which TRAPUSR1' or 'functions TRAPUSR1'
TRAPUSR1() {
  # Display stderr output if available
  if [[ -n $__DIRENV_INSTANT_STDERR_FILE ]] && [[ -f $__DIRENV_INSTANT_STDERR_FILE ]]; then
    if [[ -s $__DIRENV_INSTANT_STDERR_FILE ]]; then
      printf '%s\n' "$(<"$__DIRENV_INSTANT_STDERR_FILE")"
    fi
    command rm -f "$__DIRENV_INSTANT_STDERR_FILE" 2>/dev/null || true
    __DIRENV_INSTANT_STDERR_FILE=""
  fi

  # Load environment variables (keep file as cache for next time)
  if [[ -n $__DIRENV_INSTANT_ENV_FILE ]] && [[ -f $__DIRENV_INSTANT_ENV_FILE ]]; then
    eval "$(<"$__DIRENV_INSTANT_ENV_FILE")"
  fi

  # Chain to previous handler if one existed
  (( $+functions[__direnv_instant_orig_TRAPUSR1] )) && __direnv_instant_orig_TRAPUSR1 "$@"

  # Redraw the prompt after receiving output from direnv
  # This ensures the prompt is displayed after async output
  zle && zle .reset-prompt && zle -R
}

# Main hook called on directory changes and prompts
_direnv_hook() {
  export DIRENV_INSTANT_SHELL_PID=$$

  # Load cached environment immediately if available and caching is enabled
  if [[ ${DIRENV_INSTANT_USE_CACHE:-1} == 1 ]] && [[ -n $__DIRENV_INSTANT_ENV_FILE ]] && [[ -f $__DIRENV_INSTANT_ENV_FILE ]]; then
    eval "$(<"$__DIRENV_INSTANT_ENV_FILE")"
  fi

  trap -- '' SIGINT
  eval "$(direnv-instant start)"
  trap - SIGINT
}

# Cleanup on shell exit
_direnv_exit_cleanup() {
  direnv-instant stop
}

# Initialize hooks if not already done
if [[ -z ${__DIRENV_INSTANT_HOOKED} ]]; then
  typeset -g __DIRENV_INSTANT_HOOKED=1

  # Register zsh hooks
  autoload -Uz add-zsh-hook
  add-zsh-hook precmd _direnv_hook
  add-zsh-hook chpwd _direnv_hook
  add-zsh-hook zshexit _direnv_exit_cleanup

  # Run initial hook
  _direnv_hook
fi
