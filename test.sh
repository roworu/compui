#!/bin/sh
# builds, then starts compUI in a pure zsh (no personal rc files, so no tzc etc.) with the host's debug log on
set -e
cd "$(dirname "$0")"
./build.sh
pgrep -x compui-host | xargs kill 2>/dev/null || true # a running host would be the old build
export ZDOTDIR="$(mktemp -d)"
trap 'rm -rf "$ZDOTDIR"' EXIT
echo "host log: tail -f /tmp/compui-host.log; type 'git ch' below"
clear # the wrapper assumes the cursor starts at row 0, true only on a fresh screen
./dist/compui
