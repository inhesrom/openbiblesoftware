#!/usr/bin/env bash
# PROTOTYPE (#374): run the rendering test at 1x and 2x. gpui-pre 0.3.12's headless windows
# are fixed at 2x, so this copies gpui-pre from Cargo's registry into target/, applies a
# 19-line patch that lets a headless window change its scale (the same call TestAppContext
# already has), and builds against the copy with `--config patch...` and `--features scale`.
# Cargo.lock is restored afterwards.
set -euo pipefail
cd "$(dirname "$0")"
cargo fetch -q
src=$(ls -d "${CARGO_HOME:-$HOME/.cargo}"/registry/src/*/gpui-pre-0.3.12 | head -1)
dst=target/patched/gpui-pre-0.3.12
rm -rf "$dst"; mkdir -p target/patched; cp -r "$src" "$dst"; chmod -R u+w "$dst"
patch -s -p1 -d "$dst" < patches/gpui-pre-headless-scale.patch
cp Cargo.lock target/Cargo.lock.unpatched
trap 'cp target/Cargo.lock.unpatched Cargo.lock' EXIT
cargo --config "patch.crates-io.gpui-pre.path=\"$dst\"" test --release --features scale --test rendering "$@"
