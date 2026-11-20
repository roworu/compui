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

To uninstall (removes the binary, that line, the GNOME extension, config, runtime and cache files):
```sh
curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh -s -- --uninstall
```

## Config

Optional, `~/.config/compui/config`, picked up on change:
```ini
enabled = on            # popup at all
width = 40              # popup width in cells; longer labels are cut, the full description shows below
terminal.konsole = off  # turn compUI off in one terminal: ghostty, konsole, alacritty; macOS: terminal, iterm2, generic
dark.bg = #2f343e       # colors per scheme (dark. / light.): bg, border, selected, text, muted, scrollbar,
light.folder = #0184bc  # and per kind: subcommand, suggestion, option, command, folder
debug = off             # log to $XDG_RUNTIME_DIR/compui/host.log
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
