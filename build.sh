#!/bin/sh
# builds dist/{compui-host,compui-pty,compui}; run ./dist/compui to start the host if needed and a wrapped shell
set -e
cd "$(dirname "$0")"
export PATH="$PATH:/opt/homebrew/bin:$HOME/.cargo/bin"
mkdir -p dist; rm -f dist/compui-pty dist/compui-host dist/compcore # overwriting a signed binary in place gets it killed
cargo build --release -q
cp target/release/compui-pty dist/
# per os and arch: target/ may be synced between machines
cc="target/compcore/$(uname -s)-$(uname -m)"
[ -x "$cc/bin/compcore" ] || cargo install --git https://github.com/roworu/compcore --tag v0.1.3 compcore --root "$cc" -q
cp "$cc/bin/compcore" dist/
mkdir -p dist/shell && cp -R shell/zsh dist/shell/
# host: swift on macos, rust (crates/compui-host) on linux
if [ "$(uname)" = Darwin ]; then
  swiftc -O macos/common/*.swift macos/ghostty/*.swift macos/terminal/*.swift macos/iterm2/*.swift macos/generic/*.swift -o dist/compui-host 2>/dev/null
else
  cp target/release/compui-host dist/
fi
cat > dist/compui <<'L'
#!/bin/sh
d="$(cd "$(dirname "$0")" && pwd)"
# a running host whose socket doesn't answer is stale: replace it
if ! nc -zU "/tmp/compui-$(id -u).sock" >/dev/null 2>&1; then
  pgrep -x compui-host | xargs kill 2>/dev/null
  (COMPUI_DEBUG=1 "$d/compui-host" 2>/tmp/compui-host.log >&2 &)
  sleep 1
fi
exec "$d/compui-pty"
L
chmod +x dist/compui
