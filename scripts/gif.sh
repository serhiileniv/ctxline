#!/usr/bin/env bash
# Renders the README GIF: the status line while a session fills the window.
#
# Every frame is the real binary's output -- this script only supplies the
# payloads and the pacing, so a palette or format change shows up here without
# anyone having to redraw anything. Regenerate with `vhs assets/demo.tape`.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
B="$ROOT/target/release/ctxline"
[[ -x "$B" ]] || { echo "no binary at $B -- run: cargo build --release" >&2; exit 1; }

WINDOW=1000000
FRAME=${FRAME:-0.07}                              # seconds; ~6s for the whole climb

D=$'\033[38;5;244m'                     # box grey, quieter than anything ctxline prints
R=$'\033[0m'

pause() { perl -e 'select undef,undef,undef,$ARGV[0]' "$1" 2>/dev/null || sleep "$1"; }

# The Claude Code footer as it actually looks with `padding: 0`: the input box,
# then the status line flush at the left edge underneath it.
box() {
  local w=58
  printf '%s╭%s╮%s\n' "$D" "$(printf '─%.0s' $(seq 1 $w))" "$R"
  printf '%s│%s > %s%-*s%s│%s\n' "$D" "$R" "$D" $((w - 3)) "$1" "$D" "$R"
  printf '%s╰%s╯%s\n' "$D" "$(printf '─%.0s' $(seq 1 $w))" "$R"
}

line() {
  printf '{"model":{"display_name":"Opus 5"},"context_window":{"context_window_size":%s,"current_usage":{"input_tokens":%s}}}' \
    "$WINDOW" "$1" | "$B"
}

printf '\033[?25l'                      # hide the cursor: it has no business in a GIF
trap 'printf "\033[?25h"' EXIT

box 'refactor the tokenizer and keep the tests green'
line 8000

# The climb. Uneven steps because real usage is uneven -- a long file read moves
# the number further than a one-word answer does.
for used in \
   8000  14000  23000  31000  52000  68000  74000 103000 128000 141000 \
 176000 194000 233000 268000 291000 317000 344000 362000 401000 428000 \
 455000 483000 512000 547000 566000 588000 604000 631000 658000 672000 \
 699000 724000 741000 768000 786000 802000 819000 837000 852000 864000 \
 878000 891000 903000 916000 928000 937000 946000 954000 961000
do
  pause "$FRAME"
  printf '\033[1A\033[2K'               # up one line, wipe it, redraw
  line "$used"
done

pause 20                                # hold on a full window; vhs cuts the recording
