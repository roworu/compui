#!/bin/sh
# installs compUI for this user on linux: builds dist/, copies it to ~/.local/lib/compui, puts compui-pty
# on PATH (~/.local/bin), runs the host as a systemd user service (compui.service, started with the
# graphical session) and writes ~/.config/compui/config if there is none. removes older installs first.
# ./install.sh --uninstall removes it all again, the config file stays
set -e
cd "$(dirname "$0")"
[ "$(uname)" = Linux ] || { echo "install.sh is for linux; on macos use ./build.sh" >&2; exit 1; }
lib="$HOME/.local/lib/compui"
bin="$HOME/.local/bin"
config="${XDG_CONFIG_HOME:-$HOME/.config}/compui/config"
unit="${XDG_CONFIG_HOME:-$HOME/.config}/systemd/user/compui.service"

# the service, and hosts started otherwise (older installs' autostart, ./test.sh); sigterm lets a host
# unload its kwin script
stop_hosts() {
  systemctl --user stop compui.service 2>/dev/null || true
  pkill -x compui-host 2>/dev/null || true
  for _ in 1 2 3 4 5 6 7 8 9 10; do pgrep -x compui-host >/dev/null || break; sleep 0.2; done
}

# what older installs and killed runs leave behind
cleanup() {
  rm -f "${XDG_CONFIG_HOME:-$HOME/.config}/autostart/compui-host.desktop"
  # the tray icon of older installs
  rm -rf "$lib/icons"
  for s in /tmp/compui-*.zsh.sock; do
    [ -S "$s" ] || continue
    pid="${s#/tmp/compui-}"; pid="${pid%.zsh.sock}"
    kill -0 "$pid" 2>/dev/null || rm -f "$s"
  done
  rm -f "/tmp/compui-$(id -u).sock" "/tmp/compui-$(id -u)-kwin.js"
}

if [ "$1" = --uninstall ]; then
  systemctl --user disable compui.service 2>/dev/null || true
  stop_hosts
  cleanup
  rm -rf "$lib" "$unit"
  systemctl --user daemon-reload
  [ -L "$bin/compui-pty" ] && rm "$bin/compui-pty"
  echo "compUI removed ($config kept). undo the terminal setup (command = … / .zshrc line) if you made it"
  exit 0
fi

./build.sh
stop_hosts
cleanup
# a fresh copy, not overwriting in place: running wrappers keep their old files
rm -rf "$lib"
mkdir -p "$(dirname "$lib")" "$bin" "$(dirname "$unit")" "$(dirname "$config")"
cp -R dist "$lib"
rm -f "$lib/compui" # the dev launcher, the service starts the host here
ln -sf "$lib/compui-pty" "$bin/compui-pty"

[ -f "$config" ] || cat > "$config" <<'EOF'
# compUI, read by compui-host; edits apply on the next keystroke

# show the completion popup
enabled = true

# log every line, window and placement: journalctl --user -u compui -f
debug = false
EOF

cat > "$unit" <<'EOF'
[Unit]
Description=compUI completion popup for terminals
PartOf=graphical-session.target
After=graphical-session.target

[Service]
ExecStart=%h/.local/lib/compui/compui-host
Restart=on-failure
RestartSec=2

[Install]
WantedBy=graphical-session.target
EOF
systemctl --user daemon-reload
systemctl --user enable --now compui.service

cat <<EOF
compUI installed in $lib; compui.service runs the host now and with every graphical session.
  config:  $config
  status:  systemctl --user status compui
  logs:    journalctl --user -u compui -f

point your terminal at the wrapper instead of the shell:
  ghostty  ~/.config/ghostty/config:  command = $bin/compui-pty
  konsole  Settings > Manage Profiles > New, Command: $bin/compui-pty, then Set as Default
or, for any terminal running zsh, at the top of ~/.zshrc:
  [[ -z \$COMPUI_SOCKET ]] && exec $bin/compui-pty
other completion menus drawn in the terminal (like tzc) should stay off under it:
  [[ -z \$COMPUI_SOCKET ]] && source …
EOF
