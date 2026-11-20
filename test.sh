#!/bin/sh
# builds, then starts compUI in a pure zsh (no personal rc files, so no tzc etc.) with the host's debug log on
set -e
cd "$(dirname "$0")"
./build.sh
export ZDOTDIR="$(mktemp -d)" COMPUI_DEBUG=1
# a running host would be the old build. it is replaced right away: wrappers in other terminals
# (the installed compui) would otherwise start their own build within 100ms
log="${XDG_RUNTIME_DIR:-$HOME/.cache}/compui/host.log"
mkdir -p "$(dirname "$log")"
pkill -f 'compui host' 2>/dev/null || true
setsid ./dist/compui host </dev/null >/dev/null 2>"$log" &
sleep 0.3
trap 'rm -rf "$ZDOTDIR"' EXIT
echo "host log: tail -f ${XDG_RUNTIME_DIR:-$HOME/.cache}/compui/host.log; type 'git ch' below"
./dist/compui
