#!/bin/sh
# installs the latest compUI release into ~/.local/bin (or $COMPUI_BIN):
#   curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh
set -e
case "$(uname -s)" in
  Linux) os=linux ;;
  Darwin) os=macos ;;
  *) echo "compUI: unsupported os $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch=x86_64 ;;
  arm64 | aarch64) arch=arm64 ;;
  *) echo "compUI: unsupported cpu $(uname -m)" >&2; exit 1 ;;
esac
bin="${COMPUI_BIN:-$HOME/.local/bin}"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -fsSL "https://github.com/roworu/compui/releases/latest/download/compui-$os-$arch.tar.gz" | tar xz -C "$tmp"
mkdir -p "$bin"
# moved over, not written in place: running wrappers keep their old file and macos keeps the signature valid
for f in "$tmp"/*; do mv -f "$f" "$bin/"; done
echo "compUI installed: $bin/compui"
[ "$os" = linux ] && echo "needs gtk4 and gtk4-layer-shell, and kde plasma on wayland for the popup"
cat <<M
point your terminal at it instead of the shell, e.g. ghostty: command = $bin/compui
or at the top of ~/.zshrc:  [[ -z \$COMPUI_SOCKET ]] && exec $bin/compui
M
