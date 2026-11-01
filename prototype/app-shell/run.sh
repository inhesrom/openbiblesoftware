#!/usr/bin/env sh
# PROTOTYPE (#13): one command to run. First run downloads open sources (~50 MB) into .cache/.
set -e
cd "$(dirname "$0")"
if [ ! -f data/kjv.json ] || [ ! -f assets/fonts/SILEOT.ttf ]; then
  python3 prep_data.py
fi
exec cargo run --release -- "$@"
