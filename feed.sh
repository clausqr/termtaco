#!/usr/bin/env bash
# feed.sh — simple test feeder for gauge.
#
# Generates a stream of numbers on stdout for piping into gauge, e.g.:
#     ./feed.sh sine | cargo run --release
#     ./feed.sh sine | ./target/release/gauge --window 100
#
# Modes:
#   sine    smooth oscillation around 50 (default) — best for watching the needle
#   noisy   sine plus random jitter — exercises the ±1σ band
#   ramp    slow climb then reset — sweeps the dial end to end
#   random  uniform noise in [0, 100)
#   rate    mimics the real producer: "average rate: <n>" lines (tests lenient parse)
#
# Args:  ./feed.sh [mode] [delay_seconds]
set -euo pipefail

mode="${1:-sine}"
delay="${2:-0.05}"

# Fractional sleep needs GNU/BSD sleep; fail clearly on minimal implementations.
if ! sleep 0.01 2>/dev/null; then
    echo "feed.sh: needs a sleep that accepts fractional seconds (GNU/BSD)" >&2
    exit 1
fi

i=0
while true; do
    case "$mode" in
        sine)
            v=$(awk -v i="$i" 'BEGIN { printf "%.4f", 50 + 40*sin(i/15) }')
            printf '%s\n' "$v"
            ;;
        noisy)
            v=$(awk -v i="$i" -v r="$RANDOM" 'BEGIN { printf "%.4f", 50 + 30*sin(i/15) + (r/32768-0.5)*20 }')
            printf '%s\n' "$v"
            ;;
        ramp)
            v=$(awk -v i="$i" 'BEGIN { printf "%.4f", (i % 100) }')
            printf '%s\n' "$v"
            ;;
        random)
            awk -v r="$RANDOM" 'BEGIN { printf "%.4f\n", r/32768*100 }'
            ;;
        rate)
            v=$(awk -v i="$i" 'BEGIN { printf "%.3f", 33 + 5*sin(i/20) }')
            printf 'average rate: %s\n' "$v"
            ;;
        *)
            echo "feed.sh: unknown mode '$mode' (sine|noisy|ramp|random|rate)" >&2
            exit 2
            ;;
    esac
    i=$((i + 1))
    sleep "$delay"
done
