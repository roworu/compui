#!/bin/sh
# builds dist/compui; on macos build.rs links the swift host into it (needs swiftc)
set -e
cd "$(dirname "$0")"
export PATH="$PATH:/opt/homebrew/bin:$HOME/.cargo/bin"
mkdir -p dist; rm -f dist/compui # overwriting a signed binary in place gets it killed
cargo build --release -q
cp target/release/compui dist/
