# compUI

IDE style completion popup for terminals you already use (ghostty, konsole, Terminal.app, iTerm2...). Type a command, pick a candidate with up/down, tab to insert. Recent history lines that start with what you typed are listed first.

zsh only.

![compUI in ghostty and konsole](assets/demo.gif)

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh
```

It adds this line to the top of your .zshrc (skipped if already there):
```bash
[[ -z $COMPUI_SOCKET ]] && exec ~/.local/bin/compui
```

## OS support
Linux: KDE Plasma on Wayland, needs gtk4 and gtk4-layer-shell.
macOS: allow your terminal in Accessibility settings.
