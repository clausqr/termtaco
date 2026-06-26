#!/usr/bin/env bash
# feed.sh — simple test feeder for termtaco.
#
# Generates a stream of numbers on stdout for piping into termtaco, e.g.:
#     ./feed.sh sine | cargo run --release
#     ./feed.sh sine | ./target/release/termtaco --window 100
#
# Modes:
#   sine    smooth oscillation around 50 (default) — best for watching the needle
#   noisy   sine plus random jitter — exercises the ±1σ band
#   ramp    slow climb then reset — sweeps the dial end to end
#   random  uniform noise in [0, 100)
#   rate    mimics the real producer: "average rate: <n>" lines (tests lenient parse)
#   burst   emit for `on` seconds then go quiet for `off` seconds, looping —
#           keeps stdin open while quiet, so the gauge goes STALE then recovers
#
# Args:  ./feed.sh [mode] [delay_seconds] [on_seconds] [off_seconds]
#        on/off apply to `burst` only (defaults: on=4, off=5; off > the gauge's
#        3s stale threshold so STALE shows during the quiet window).
set -euo pipefail

mode="${1:-sine}"
delay="${2:-0.05}"
on_secs="${3:-4}"
off_secs="${4:-5}"

# Fractional sleep needs GNU/BSD sleep; fail clearly on minimal implementations.
if ! sleep 0.01 2>/dev/null; then
    echo "feed.sh: needs a sleep that accepts fractional seconds (GNU/BSD)" >&2
    exit 1
fi

# Burst phase lengths, in loop iterations (each iteration sleeps `delay`).
on_n=$(awk -v x="$on_secs" -v d="$delay" 'BEGIN { n = int(x / d + 0.5); if (n < 1) n = 1; print n }')
off_n=$(awk -v x="$off_secs" -v d="$delay" 'BEGIN { n = int(x / d + 0.5); if (n < 1) n = 1; print n }')
cycle=$((on_n + off_n))

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
        burst)
            # Emit only during the "on" part of the cycle; stay quiet (but keep
            # the pipe open via the trailing sleep) during the "off" part.
            if [ "$((i % cycle))" -lt "$on_n" ]; then
                v=$(awk -v i="$i" 'BEGIN { printf "%.4f", 50 + 40*sin(i/15) }')
                printf '%s\n' "$v"
            fi
            ;;
        *)
            echo "feed.sh: unknown mode '$mode' (sine|noisy|ramp|random|rate|burst)" >&2
            exit 2
            ;;
    esac
    i=$((i + 1))
    sleep "$delay"
done
