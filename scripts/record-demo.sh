#!/usr/bin/env nix-shell
#!nix-shell -i bash -p tmux asciinema asciinema-agg direnv
# Records a demo of direnv-instant using tmux send-keys for real interactive behavior
set -e

DEMO_DIR="/tmp/demo-direnv-instant"
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
CAST_FILE="$SCRIPT_DIR/demo.cast"
GIF_FILE="$SCRIPT_DIR/demo.gif"
SESSION="demo-$$"
DIRENV_INSTANT="$SCRIPT_DIR/result/bin/direnv-instant"

# Setup demo project with slow .envrc
echo "Setting up demo project..."
rm -rf "$DEMO_DIR"
mkdir -p "$DEMO_DIR"
cat > "$DEMO_DIR/.envrc" << 'EOF'
echo "⏳ Loading environment..." >&2
sleep 2
echo "📦 Installing dependencies..." >&2
sleep 2
echo "🔨 Building..." >&2
sleep 2
echo "✅ Ready!" >&2
export DEMO_VAR="Hello from direnv-instant!"
EOF
direnv allow "$DEMO_DIR" 2>/dev/null

type_keys() {
    local text="$1" delay="${2:-0.04}"
    for ((i=0; i<${#text}; i++)); do
        tmux send-keys -t "$SESSION" -l "${text:$i:1}"
        sleep "$delay"
    done
}

send_enter() {
    tmux send-keys -t "$SESSION" Enter
    sleep "${1:-0.5}"
}

# Cleanup any existing session
tmux kill-session -t "$SESSION" 2>/dev/null || true

# Create tmux session with clean bash
echo "Starting recording..."
DIRENV_LOG_FORMAT='' tmux new-session -d -s "$SESSION" -x 100 -y 30 "bash --norc --noprofile"
sleep 0.5

# Start asciinema recording
asciinema rec --overwrite --cols 100 --rows 30 "$CAST_FILE" --command "tmux attach -t $SESSION" &
ASCIINEMA_PID=$!
sleep 1

# Initialize shell
tmux send-keys -t "$SESSION" "PS1='$ '" Enter
sleep 0.3
tmux send-keys -t "$SESSION" "cd /tmp && clear" Enter
sleep 0.5

# Demo script
type_keys "# direnv-instant: Instant shell prompts with async direnv" 0.03
send_enter
sleep 2

type_keys "# Enable the direnv-instant hook:" 0.03
send_enter 0.5
type_keys "eval \"\$($DIRENV_INSTANT hook bash)\"" 0.02
send_enter 1

type_keys "export DIRENV_INSTANT_MUX_DELAY=2" 0.03
send_enter 0.5

send_enter
type_keys "# Now cd into a project with a slow .envrc..." 0.03
send_enter 1

type_keys "cd $DEMO_DIR" 0.03
send_enter 0.3

send_enter
type_keys "# Prompt returned INSTANTLY!" 0.03
send_enter
type_keys "# Watch: a tmux pane shows progress below..." 0.03
send_enter

sleep 12  # Wait for direnv to complete

send_enter
type_keys "# Environment loaded! Verify:" 0.03
send_enter 0.5
type_keys "echo \"DEMO_VAR = \$DEMO_VAR\"" 0.03
send_enter 2

send_enter
type_keys "# That is direnv-instant!" 0.03
send_enter
sleep 3

# End recording
tmux send-keys -t "$SESSION" "exit" Enter
sleep 1
wait $ASCIINEMA_PID 2>/dev/null || true
tmux kill-session -t "$SESSION" 2>/dev/null || true

echo ""
echo "Recording complete: $CAST_FILE"
echo "Converting to GIF..."
agg --theme monokai "$CAST_FILE" "$GIF_FILE"
echo "GIF created: $GIF_FILE"
