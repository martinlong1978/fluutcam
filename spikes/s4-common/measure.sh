#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Spike S4 measurements for both prototypes. Run from spikes/s4-common with a
# display (on a headless machine: Xvfb, DISPLAY=:99) after building both
# prototypes in release mode. Prints one block per prototype.
#
#   ./measure.sh            all but the clean build
#   ./measure.sh build      clean release build times only (slow)
#
# If s4-slint was also built with `--features skia` into target-skia
# (CARGO_TARGET_DIR=target-skia), that build is measured as well.
set -euo pipefail
cd "$(dirname "$0")/.."
export SLINT_WGPU_CPU=1   # allow slint on a CPU adapter (lavapipe)
apps=(s4-egui s4-slint)

median() { sort -n | awk '{a[NR]=$1} END {print a[int((NR+1)/2)]}'; }

if [[ "${1:-}" == build ]]; then
    for app in "${apps[@]}"; do
        (cd "$app" && cargo clean -q && s=$(date +%s.%N) && cargo build --release -q &&
            echo "$app clean_build_s=$(echo "$(date +%s.%N) - $s" | bc)")
    done
    exit 0
fi

runs=("s4-egui:s4-egui/target/release/s4-egui" "s4-slint:s4-slint/target/release/s4-slint")
[[ -x s4-slint/target-skia/release/s4-slint ]] &&
    runs+=("s4-slint-skia:s4-slint/target-skia/release/s4-slint")
for run in "${runs[@]}"; do
    app=${run%%:*}
    bin=${run#*:}
    echo "== $app"
    echo "frame_1M: $(S4_BENCH=${FRAMES:-100} "$bin" 2>/dev/null | grep frames=)"
    echo "frame_small: $(S4_GRID=50 S4_BENCH=${FRAMES:-100} "$bin" 2>/dev/null | grep frames=)"
    echo "frame_small_tree200: $(S4_GRID=50 S4_TREE=10x20 S4_BENCH=${FRAMES:-100} "$bin" 2>/dev/null | grep frames=)"
    for grid in 708 50; do
        runs=$(for _ in 1 2 3 4 5; do S4_GRID=$grid S4_STARTUP=1 "$bin" 2>/dev/null | sed -n 's/startup_ms=//p'; done)
        echo "startup_ms_grid$grid: median $(echo "$runs" | median) of: $(echo $runs)"
    done
    # Idle CPU: start, let it settle, then count CPU time over 10 s.
    "$bin" >/dev/null 2>&1 &
    pid=$!
    sleep 6
    tick=$(getconf CLK_TCK)
    t1=$(awk '{print $14 + $15}' /proc/$pid/stat)
    sleep 10
    t2=$(awk '{print $14 + $15}' /proc/$pid/stat)
    rss=$(awk '/VmRSS/ {print $2}' /proc/$pid/status)
    kill $pid; wait $pid 2>/dev/null || true
    echo "idle_cpu_pct=$(echo "scale=2; ($t2 - $t1) * 100 / $tick / 10" | bc) idle_rss_mb=$((rss / 1024))"
    cp "$bin" /tmp/$app.stripped && strip /tmp/$app.stripped
    echo "binary_bytes=$(stat -c %s "$bin") stripped_bytes=$(stat -c %s /tmp/$app.stripped)"
    echo "crates=$(cd "${app%-skia}" && cargo tree --prefix none -e normal --target all 2>/dev/null | sort -u | wc -l) (normal deps, all targets)"
done

loc() { grep -cvE '^\s*(//.*)?$' "$@" | awk -F: '{s += $NF} END {print s}'; }
echo "== lines of code (non-blank, non-comment)"
echo "s4-egui: rust=$(loc s4-egui/src/*.rs)"
echo "s4-slint: rust=$(loc s4-slint/src/*.rs s4-slint/build.rs) slint=$(loc s4-slint/ui/*.slint)"
echo "s4-common (shared): rust=$(loc s4-common/src/*.rs) wgsl=$(loc s4-common/src/*.wgsl)"
