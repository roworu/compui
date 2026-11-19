# compUI

Fig-style completion popup for terminals you already use (ghostty, konsole, Terminal.app, iTerm2). Type a command, pick a candidate with up/down, tab to insert.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh
```

Then set your terminal's command to `~/.local/bin/compui` (ghostty: `command = ~/.local/bin/compui`).

Linux: KDE Plasma on Wayland, needs gtk4 and gtk4-layer-shell. macOS: allow your terminal in Accessibility settings. zsh only.
