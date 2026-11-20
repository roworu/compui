# compUI

IDE style completion popup for terminals you already use. Type a command, pick a candidate with up/down, tab to insert. Recent history lines that start with what you typed are listed first.

zsh only.

![compUI in ghostty and konsole](assets/demo.gif)

## Install / update

```sh
curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh
```

It adds this line to the top of your .zshrc (skipped if already there):
```bash
[[ -z $COMPUI_SOCKET ]] && exec ~/.local/bin/compui
```

## OS support
Linux: KDE Plasma or GNOME on Wayland, needs gtk4 and gtk4-layer-shell. On GNOME (46+) compUI installs and enables a small shell extension on first run, since GNOME lets no app place a popup or see window positions; log out and back in once so GNOME loads it.
macOS: allow your terminal in Accessibility settings.

## Supported terminals

| Terminal | Linux | macOS |
| --- | --- | --- |
| ghostty | [yes](linux/ghostty/main.rs) | [yes](macos/ghostty/ghostty.swift) |
| konsole | [yes](linux/konsole/main.rs) | — |
| alacritty | [yes](linux/alacritty/main.rs) | [yes](macos/alacritty/alacritty.swift) |
| Terminal.app | — | [yes](macos/terminal/terminal.swift) |
| iTerm2 | — | [yes](macos/iterm2/iterm2.swift) |
| others | — | [best effort](macos/generic/generic.swift) |
