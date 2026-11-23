#!/usr/bin/env bash
# PROTOTYPE (#374): every test with no display server at all, then the 1x/2x rendering run.
set -euo pipefail
cd "$(dirname "$0")"
export OBS_PROTO_NO_DISPLAY=1
unset DISPLAY WAYLAND_DISPLAY
cargo test --release -j "${JOBS:-5}" --test interaction -- --test-threads=1 --nocapture
cargo test --release -j "${JOBS:-5}" --test rendering
./scale.sh -j "${JOBS:-5}"
