#!/bin/sh
# installs the latest compUI release into ~/.local/bin and starts it from ~/.zshrc:
#   curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh
set -e
os=linux; [ "$(uname -s)" = Darwin ] && os=macos
arch="$(uname -m)"; [ "$arch" = aarch64 ] && arch=arm64
bin="$HOME/.local/bin"
mkdir -p "$bin"
curl -fsSL "https://github.com/roworu/compui/releases/latest/download/compui-$os-$arch.tar.gz" | tar xzO ./compui > "$bin/compui.new"
chmod +x "$bin/compui.new"
# moved over, not written in place: running wrappers keep their old file and macos keeps the signature valid
mv -f "$bin/compui.new" "$bin/compui"
echo "compUI installed: $bin/compui"

rc="${ZDOTDIR:-$HOME}/.zshrc"
line="[[ -z \$COMPUI_SOCKET ]] && exec $bin/compui"
if grep -qs 'COMPUI_SOCKET' "$rc"; then
  echo "$rc already starts compUI, left as is"
else
  # on top so the outer shell hands over before loading the rest; written through, not moved, to keep a symlinked rc
  { echo "$line"; cat "$rc" 2>/dev/null || true; } > "$rc.compui"
  cat "$rc.compui" > "$rc" && rm "$rc.compui"
  echo "added to the top of $rc: $line"
fi
