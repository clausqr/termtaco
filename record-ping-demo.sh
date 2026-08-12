#!/usr/bin/env bash
# record-ping-demo.sh: records a live side-by-side demo, termtaco
# (--parser ping --kalman) in a 1/3-width left tmux pane, real ping feeding
# it from a 2/3-width right pane, for the README's demo assets.
#
# Requires: tmux, asciinema (https://asciinema.org), termtaco itself on PATH
# (cargo install --path . from the repo root). Convert the recording to a
# GIF afterward with agg (https://github.com/asciinema/agg):
#
#     ./record-ping-demo.sh
#     agg ping-demo.cast assets/demo-ping.gif
#
# Usage: ./record-ping-demo.sh [--overwrite|--append] [host] [output.cast]
set -euo pipefail

if ! command -v tmux >/dev/null; then
    echo "record-ping-demo.sh: needs tmux" >&2
    exit 1
fi
if ! command -v asciinema >/dev/null; then
    echo "record-ping-demo.sh: needs asciinema (https://asciinema.org)" >&2
    exit 1
fi

# Leading --flags pass straight through to `asciinema rec` (e.g. --overwrite
# to re-record over an existing .cast, --append to add to one); anything
# else is the positional host/output pair below.
asciinema_opts=()
while [[ "${1:-}" == --* ]]; do
    asciinema_opts+=("$1")
    shift
done

host="${1:-8.8.8.8}"
out="${2:-ping-demo.cast}"
session="termtaco-demo-$$"
fifo="$(mktemp -u /tmp/termtaco-ping-XXXXXX.fifo)"

cleanup() {
    tmux kill-session -t "$session" 2>/dev/null || true
    rm -f "$fifo"
}
trap cleanup EXIT INT TERM

mkfifo "$fifo"

# A dedicated, detached session, so this never touches whatever tmux
# session you're already working in. Sized to *this* terminal explicitly:
# asciinema attaches to it from right here in a moment, and if the session's
# size doesn't already match, tmux has to resize the whole layout to fit on
# attach, which does not preserve the 1/3:2/3 split computed below.
#
# `resize-window -x/-y` also flips this session's `window-size` option to
# `manual` (scoped to this session only, doesn't touch your tmux config),
# which matters on a server where `window-size` is `latest` (tmux's own
# default since 2.9): without it, a *second* resize kicks in on attach,
# snapping to whatever other client most recently had focus, undoing the
# size we just set here.
cols="$(tput cols 2>/dev/null || echo 200)"
lines="$(tput lines 2>/dev/null || echo 50)"

tmux new-session -d -s "$session" -x "$cols" -y "$lines"

# No status bar for the recording: nothing worth showing there for a demo,
# and it means the window can use the terminal's full height instead of
# reserving a line for it. Scoped to this session only, doesn't touch your
# tmux config.
tmux set-option -t "$session" status off

tmux resize-window -t "$session" -x "$cols" -y "$lines"

# Side-by-side split (-h = left/right in tmux's terms), left pane at 1/3
# width. `split-window -l` sizes the *new* pane, and the new pane lands to
# the right of the one it's split from, so `-l 67%` leaves the original
# (left) pane at ~1/3 and gives the new (right) pane ~2/3.
#
# Addressed by pane ID (e.g. %12), not window index: tmux's default window
# numbering starts at 0, but plenty of configs (including whatever wrote
# this comment's own test box) set `base-index 1`, which silently breaks a
# hardcoded `:0`.
left_pane="$(tmux list-panes -t "$session" -F '#{pane_id}')"
right_pane="$(tmux split-window -h -l 67% -t "$left_pane" -P -F '#{pane_id}')"

# Left pane (1/3): termtaco, blocked quietly on opening the FIFO until the
# right pane's writer (armed below) connects.
tmux send-keys -t "$left_pane" \
    "termtaco --parser ping --title 'ping ms' --theme gruvbox --0 --kalman --kalman-q 0.5 --kalman-r 1300 --needle-inertia 0.5 < '$fifo'" \
    Enter

# Right pane (2/3): fan this pane's own output into the FIFO *before*
# starting ping, so the first reply isn't lost, then run plain, unmodified
# ping. Note: pipe-pane duplicates the pane's raw rendered output, prompt
# chrome included, so there's a burst of ANSI noise in the FIFO right as
# the `ping` command line itself is echoed back by your shell; termtaco's
# --parser just skips lines it can't read, so this only shows up as a beat
# of nothing on the dial before real replies start landing, not a crash.
tmux pipe-pane -t "$right_pane" -o "cat > '$fifo'"
tmux send-keys -t "$right_pane" "ping '$host'" Enter

# Give both panes a moment to actually come up before recording starts.
sleep 1.5

echo "Recording to $out. Detach with Ctrl-b d (or Ctrl-C) when you have your take."
asciinema rec "${asciinema_opts[@]}" "$out" -c "tmux attach -t $session"

echo "Saved $out"
echo "Convert to a GIF for the README with: agg '$out' assets/demo-ping.gif"
