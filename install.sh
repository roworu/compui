#!/bin/sh
# installs the latest compUI release into ~/.local/bin and starts it from ~/.zshrc:
#   curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh
# removes it and everything it added (rc line, gnome extension, config, runtime and cache files):
#   curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh -s -- --uninstall
set -e
bin="$HOME/.local/bin"
rc="${ZDOTDIR:-$HOME}/.zshrc"
ext="${XDG_DATA_HOME:-$HOME/.local/share}/gnome-shell/extensions/compui@roworu.github.io"

if [ "$1" = --uninstall ]; then
  pkill -f "$bin/compui host" 2>/dev/null || true
  rm -f "$bin/compui"
  if grep -qs 'COMPUI_SOCKET' "$rc"; then
    # written through, not moved, to keep a symlinked rc
    grep -v 'COMPUI_SOCKET.*exec .*compui' "$rc" > "$rc.compui" || true
    cat "$rc.compui" > "$rc" && rm "$rc.compui"
    echo "removed the compUI line from $rc"
  fi
  if [ -d "$ext" ]; then
    gnome-extensions disable compui@roworu.github.io 2>/dev/null || true
    rm -rf "$ext"
    echo "removed the gnome shell extension"
  fi
  rm -rf "${XDG_CONFIG_HOME:-$HOME/.config}/compui" "${XDG_CACHE_HOME:-$HOME/.cache}/compcore" "$HOME/.cache/compui"
  [ -n "$XDG_RUNTIME_DIR" ] && rm -rf "$XDG_RUNTIME_DIR/compui"
  rm -f "/tmp/compui-$(id -u).sock" "/tmp/compui-$(id -u)-kwin.js" /tmp/compui-*.zsh.sock 2>/dev/null || true
  echo "compUI uninstalled; shells already running it keep it until they close"
  [ "$(uname -s)" = Darwin ] && echo "macOS: remove your terminal from Privacy & Security > Accessibility if it was only there for compUI"
  exit 0
fi

os=linux; [ "$(uname -s)" = Darwin ] && os=macos
arch="$(uname -m)"; [ "$arch" = aarch64 ] && arch=arm64
mkdir -p "$bin"
curl -fsSL "https://github.com/roworu/compui/releases/latest/download/compui-$os-$arch.tar.gz" | tar xzO ./compui > "$bin/compui.new"
chmod +x "$bin/compui.new"
# moved over, not written in place: running wrappers keep their old file and macos keeps the signature valid
mv -f "$bin/compui.new" "$bin/compui"
echo "compUI installed: $bin/compui"
# a host from the old binary stays up while any terminal runs compUI; open terminals reconnect to a new one,
# but on linux they can't start it themselves since their exe is now deleted
if pkill -f "$bin/compui host" 2>/dev/null; then
  while pgrep -f "$bin/compui host" >/dev/null; do sleep 0.1; done
  log="${XDG_RUNTIME_DIR:-$HOME/.cache}/compui"
  mkdir -p "$log"
  if command -v setsid >/dev/null; then (cd / && setsid "$bin/compui" host </dev/null >/dev/null 2>"$log/host.log" &); fi
  echo "restarted the running compUI host"
fi

line="[[ -z \$COMPUI_SOCKET ]] && exec $bin/compui"
if grep -qs 'COMPUI_SOCKET' "$rc"; then
  echo "$rc already starts compUI, left as is"
else
  # on top so the outer shell hands over before loading the rest; written through, not moved, to keep a symlinked rc
  { echo "$line"; cat "$rc" 2>/dev/null || true; } > "$rc.compui"
  cat "$rc.compui" > "$rc" && rm "$rc.compui"
  echo "added to the top of $rc: $line"
fi

case "$XDG_CURRENT_DESKTOP" in *GNOME*|*gnome*)
  echo "GNOME: on first run compUI adds a small shell extension ($ext) to show its popup;"
  echo "       log out and back in once after that so GNOME loads it" ;;
esac
