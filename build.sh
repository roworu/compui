#!/bin/sh
# builds dist/compui, plus on macos the swift host dist/compui-host next to it. on linux
# `cargo install --path crates/compui` is all an install needs
set -e
cd "$(dirname "$0")"
export PATH="$PATH:/opt/homebrew/bin:$HOME/.cargo/bin"
mkdir -p dist; rm -f dist/compui dist/compui-host # overwriting a signed binary in place gets it killed
cargo build --release -q
cp target/release/compui dist/
if [ "$(uname)" = Darwin ]; then
  swiftc -O macos/common/*.swift macos/ghostty/*.swift macos/terminal/*.swift macos/iterm2/*.swift macos/generic/*.swift -o dist/compui-host 2>/dev/null
fi
