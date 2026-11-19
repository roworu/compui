#!/bin/sh
# builds, then starts compUI in a pure zsh (no personal rc files, so no tzc etc.) with the host's debug log on
set -e
cd "$(dirname "$0")"
./build.sh
# a running host would be the old build; the wrapper starts the new one
pkill -f 'compui host' 2>/dev/null || true; pkill -x compui-host 2>/dev/null || true
sleep 0.3
export ZDOTDIR="$(mktemp -d)" COMPUI_DEBUG=1
trap 'rm -rf "$ZDOTDIR"' EXIT
echo "host log: tail -f ${XDG_RUNTIME_DIR:-$HOME/.cache}/compui/host.log; type 'git ch' below"
./dist/compui
