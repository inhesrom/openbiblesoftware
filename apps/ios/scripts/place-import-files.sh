#!/bin/bash
# Put the Import UI tests' files (S26) in a simulator's Files app, under "On My iPhone" (or
# "On My iPad"): a zip of USFM books, a SWORD module zip and a Resource file, all made from
# the repository's open-text test assets. Run it from the repository root, with the
# simulator booted and its Files app run once (a fresh simulator has no Files storage until
# then: `simctl launch <udid> com.apple.DocumentsApp`, as CI's ios.yml does), before
# `xcodebuild test`, and pass `TEST_RUNNER_OBS_IMPORT_FILES=1` to `xcodebuild test` so
# ImportTests run (docs/dev/ios-project.md, "Import tests").
#
#   bash apps/ios/scripts/place-import-files.sh <simulator udid>
#
# The files: `WEB excerpt.zip` (five WEB books in a folder), `KJV module.zip` (the
# ObsKjvOsis test module, mods.d/ at the top) and `KJV excerpt.obr` (`obs-cli import` of the
# KJV excerpt).
set -euo pipefail
udid="${1:?usage: place-import-files.sh <simulator udid>}"
root="$(pwd)"
assets="$root/crates/formats/tests/assets"
out="$root/target/ios/import-files"
rm -rf "$out"
mkdir -p "$out"

(cd "$assets/usfm" && /usr/bin/zip -q -X -r "$out/WEB excerpt.zip" web)
(cd "$assets/sword" && /usr/bin/zip -q -X -r "$out/KJV module.zip" \
  mods.d/obskjvosis.conf modules/texts/ztext/obskjvosis)
cargo run -q --locked -p obs-cli -- import "$assets/usfm/kjv2006" -o "$out/KJV excerpt.obr" \
  --id kjv-excerpt --title "KJV excerpt" --abbrev KJVX --lang en >/dev/null

# The Files app's local storage is the app group container of
# group.com.apple.FileProvider.LocalStorage; "File Provider Storage" in it is On My iPhone.
# Files just launched may take a moment to make it: wait up to 30 s.
groups="$HOME/Library/Developer/CoreSimulator/Devices/$udid/data/Containers/Shared/AppGroup"
storage=""
for _ in $(seq 30); do
  for meta in "$groups"/*/.com.apple.mobile_container_manager.metadata.plist; do
    if /usr/bin/plutil -extract MCMMetadataIdentifier raw "$meta" 2>/dev/null \
      | grep -qx 'group.com.apple.FileProvider.LocalStorage'; then
      storage="$(dirname "$meta")/File Provider Storage"
    fi
  done
  [ -n "$storage" ] && break
  sleep 1
done
if [ -z "$storage" ]; then
  echo "error: no Files local storage in simulator $udid (boot it, and open Files once)" >&2
  exit 1
fi
mkdir -p "$storage"
cp "$out"/* "$storage/"
echo "place-import-files: $(ls "$out" | tr '\n' ',' | sed 's/,$//') in $storage"
