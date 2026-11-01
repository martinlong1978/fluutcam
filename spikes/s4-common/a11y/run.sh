#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
#
# Dump a prototype's AT-SPI accessibility tree (Linux). Needs at-spi2-core,
# python3-pyatspi and a display. Usage, from spikes/:
#   dbus-run-session -- s4-common/a11y/run.sh s4-slint/target/release/s4-slint slint
set -euo pipefail
bin=$1
name=$2
python=${PYTHON:-python3}
/usr/libexec/at-spi-bus-launcher --launch-immediately >/dev/null 2>&1 &
sleep 1.5
# Start the registry, then enable accessibility so AccessKit activates.
"$python" -c "import pyatspi; pyatspi.Registry.getDesktop(0)"
S4_GRID=50 SLINT_WGPU_CPU=1 "$bin" >/dev/null 2>&1 &
pid=$!
sleep 4
dbus-send --session --print-reply --dest=org.a11y.Bus /org/a11y/bus \
    org.freedesktop.DBus.Properties.Set string:org.a11y.Status string:IsEnabled variant:boolean:true >/dev/null
sleep 4
"$python" "$(dirname "$0")/dump.py" "$name"
kill $pid
