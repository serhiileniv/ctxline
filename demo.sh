#!/usr/bin/env bash
# Renders ctxline in every state it can reach, through the real binary.
#
# Piping fixtures/sample.json in shows one line; this shows the whole surface at
# once, which is the only way to check that a palette or format change still
# works at the edges. Colour and weight are real terminal output, so what you
# see here is what the footer gets.
set -uo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
B="$ROOT/target/release/ctxline"

if [[ ! -x "$B" ]]; then
  echo "no binary at $B -- run: cargo build --release" >&2
  exit 1
fi

D=$'\033[38;5;244m'   # label grey, deliberately quieter than anything ctxline prints
R=$'\033[0m'

head() { printf '\n%s── %s %s%s\n\n' "$D" "$1" "$(printf '─%.0s' $(seq 1 $((46 - ${#1}))))" "$R"; }
pay()  { printf '%s' "$1" | "$B"; }
fill() { # display_name, window, used
  printf '{"model":{"display_name":"%s"},"context_window":{"context_window_size":%s,"current_usage":{"input_tokens":%s}}}' \
    "$1" "$2" "$3" | "$B"
}

head "filling up: 1M window"
for u in 8000 40238 210000 460000 604000 780000 862000 961000; do
  printf '   %s%3d%%%s   ' "$D" $((u * 100 / 1000000)) "$R"; fill "Opus 5" 1000000 "$u"
done

head "filling up: 200k window"
for u in 4200 38000 96000 121000 155000 178000 194000; do
  printf '   %s%3d%%%s   ' "$D" $((u * 100 / 200000)) "$R"; fill "Sonnet 5" 200000 "$u"
done

head "the three tiers"
printf '   %sgreen      <60%%%s  ' "$D" "$R"; fill "Opus 5" 200000 40238
printf '   %syellow     >60%%%s  ' "$D" "$R"; fill "Opus 5" 200000 130000
printf '   %sbold rose  >85%%%s  ' "$D" "$R"; fill "Opus 5" 200000 180000
printf '\n   %sthe top tier is bold as well as coloured: hue alone cannot separate%s\n' "$D" "$R"
printf '   %sthree states for a red/green colour-deficient reader.%s\n' "$D" "$R"

head "degraded states"
printf '   %sfresh / post-compact  %s' "$D" "$R"
pay '{"model":{"display_name":"Opus 5"},"context_window":{"context_window_size":200000}}'
printf '   %sno display_name       %s' "$D" "$R"
pay '{"model":{"id":"claude-sonnet-5"},"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":62000}}}'
printf '   %sno window size        %s' "$D" "$R"
pay '{"model":{"display_name":"Opus 5"},"context_window":{"current_usage":{"input_tokens":950000}}}'
printf '   %sname is only a marker %s' "$D" "$R"
pay '{"model":{"display_name":" (1M context)"},"context_window":{"context_window_size":1000000,"current_usage":{"input_tokens":40238}}}'
printf '   %sempty payload         %s' "$D" "$R"
pay '{}'
printf '   %sgarbage in            %s' "$D" "$R"
pay 'not json at all'
printf '\n   %severy one stays legible: nothing blank, nothing overclaiming.%s\n' "$D" "$R"

head "NO_COLOR"
printf '   %sNO_COLOR=1  (bold goes with the colour)  %s' "$D" "$R"
printf '{"model":{"display_name":"Opus 5"},"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":180000}}}' \
  | NO_COLOR=1 "$B"
printf '   %sNO_COLOR=   (set but empty -- colour stays, per no-color.org)  %s' "$D" "$R"
printf '{"model":{"display_name":"Opus 5"},"context_window":{"context_window_size":200000,"current_usage":{"input_tokens":180000}}}' \
  | NO_COLOR= "$B"
printf '\n'
