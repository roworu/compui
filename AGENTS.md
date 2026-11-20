# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

compUI: a fig-style completion popup for terminals it doesn't own, on macOS and Linux (KDE Wayland). Candidates come from [compcore](https://github.com/roworu/compcore) (pinned tag v0.1.3); compUI finds the cursor on screen, shows a native popup and puts the picked candidate into the shell line.

## Commands

- `install.sh` — downloads the latest release (`curl -fsSL https://raw.githubusercontent.com/roworu/compui/main/install.sh | sh`) into `~/.local/bin`: one `compui` binary (wrapper, host and compcore linked from the pinned tag, zsh files embedded). Point the terminal's command at it; the first `compui` starts `compui host` in the background (own session, single instance via a lock), which exits 30s after the last wrapper closes. Config `~/.config/compui/config` (`enabled`, `debug`; optional, reread on change). Runtime files (zsh adapter, `host.log`, `host.lock`) in `$XDG_RUNTIME_DIR/compui` (else `~/.cache/compui`).
- `./build.sh` — builds `dist/compui`. On macOS `crates/compui/build.rs` compiles the Swift host into a static library linked into it, so `compui host` is the host there too (it gets candidates from `compui serve`). Needs cargo, and swiftc on macOS.
- Releases: pushing a `v*` tag runs `.github/workflows/release.yml` (tests, `./build.sh` on linux x86_64/arm64 and macOS arm64, `compui-<os>-<arch>.tar.gz` of `dist/` attached to a GitHub release).
- `./test.sh` — builds, kills a running host, then starts the wrapper in a pure zsh (temp empty `ZDOTDIR`) with `COMPUI_DEBUG=1`, so the host it starts logs everything. Interactive and manual: type `git ch`. Host log: `$XDG_RUNTIME_DIR/compui/host.log`.
- `cargo test` — tests in `compui-proto` (message round-trip) and `compui` (keys, reply and config parsing). Single test: `cargo test -p compui <name>`.
- The popup needs the macOS accessibility permission and a real terminal; there are no automated UI tests.

## Testing in Terminal.app

1. System Settings → Privacy & Security → Accessibility: allow Terminal. The host is started from it and inherits the permission.
2. In Terminal.app, run `./test.sh` (builds, restarts the host, opens a pure zsh under the wrapper).
3. In a second window or tab: `tail -f ~/.cache/compui/host.log`.
4. In the first one type `git ch`. The popup should sit under the word `ch`, one cell below the cursor.
5. Check up/down and the mouse wheel move the selection, tab inserts the candidate, a history line (on top, after running `git checkout main` once) replaces the line, esc hides it until the next word, and switching to another app hides it.
6. Repeat with a second tab, a resized window and a different font size; the popup must follow the cursor.
7. If it is off, send the `window … cell … origin` log line. A line without it means `AXBoundsForRange` worked (exact).

## Architecture

One binary, two roles, talking over unix sockets:

```
terminal ⇄ compui (wrapper) ⇄ zsh (shell/zsh/compui.zsh)
              └── /tmp/compui-<uid>.sock ──► compui host (linux, compcore linked)
                                             or the Swift host (macos/, linked in; spawns `compui serve`)
```

- `crates/compui/src/pty.rs`: the wrapper, `compui` without arguments. Starts the host if the socket doesn't answer. Runs the shell in a pty and passes all bytes through untouched. Parses output with `vt100` to track the cursor cell and alt screen, owns a per-session socket for the zsh adapter, and takes up/down/tab/esc itself only while the host reports the popup `shown`. The wrapper is in every byte's path, so keep it thin (no engine, no UI).
- `shell/zsh/compui.zsh`: port of kuterm's completion adapter (protocol v1). Embedded in the binary and written to the runtime dir, injected via `ZDOTDIR`, reports each edit, applies `accept` lines with zle (dropped if `seq` is stale).
- `crates/compui-proto`: serde messages pty ⇄ host as JSON lines (`hello`, `line`, `key`, `shown`, `hidden`, `accept`, `end`), `"v":1`. The Swift host reimplements this by hand, so change both sides together.
- `macos/common/`: the Swift host (`main.swift`: sockets, compcore, tray, `NSPanel`) and the accessibility helpers plus the `Handler` protocol (`ax.swift`). Per-terminal placement lives in `macos/ghostty/` (padding read from its config, default 2), `macos/terminal/`, `macos/iterm2/`, `macos/alacritty/` (title bar, padding and dynamic padding from its toml) and `macos/generic/` (fallback); `handlers` in `main.swift` picks the first whose `matches` the frontmost bundle id. `build.rs` compiles them with `swiftc` into a static library; `compui host` calls its `compui_host_main` after taking the host lock. Like on Linux it reads the config file and exits 30s after the last wrapper; it also has a tray.

## Hosts

On macOS the host is Swift (linked into `compui`); on Linux (KDE Wayland) it is Rust (`crates/compui/src/main.rs`, which pulls in `linux/common/main.rs`, `linux/ghostty/main.rs`, `linux/konsole/main.rs` and `linux/alacritty/main.rs` via `#[path]`, popup via gtk4-layer-shell drawn with cairo/pango in `linux/common/popup.rs` (the Swift twin is `macos/common/popup.swift`, keep their look in sync), window position from a KWin script over D-Bus, compcore linked). The pty uses `vt100`. On Linux there is no tray; the host is started on demand by the wrapper and configured by one file. Cargo workspace edition is 2024.

## Editor terminals (Zed, VS Code): not supported yet

Checked on KDE Wayland. The wrapper and completions work inside both, but the host can't place the popup: KWin reports the whole editor window, and the terminal is one panel inside it at an unknown spot.

- Zed (`zed-editor`): the pty pixel size is exact (cols × floor(cell width), rows × line height). The grid starts one cell (a gutter) in from the pane's left edge; leftover space goes right, and to the bottom unless the output fills the screen (then to the top). Dock visibility is saved in `~/.local/share/zed/db/0-stable/db.sqlite` (`workspaces`), dock sizes are not. Zed doesn't join AT-SPI even with `org.a11y.Status IsEnabled` on. Only a geometry guess is left (say the right dock, with title/tab/status bar heights from `ui_font_size`), which can be off by a row.
- VS Code (`code-oss`): the pixel size is in CSS px before zoom (8.33×20.8 reported vs 12×30 on screen at zoom ×1.44). It does join AT-SPI when `IsEnabled` is on; still to check whether terminal rows are exposed with `editor.accessibilitySupport: "on"`. On Wayland AT-SPI extents are window-relative only, so they'd be combined with KWin's window rect.

## Not done yet

- host: keep Swift on macOS or move it to Rust like Linux; pty on `alacritty_terminal` instead of `vt100`
- macOS: `macos/iterm2` and `macos/alacritty` untested; multiple screens, spaces, full screen; `.app`, universal, sign, notarize, launch agent; guided accessibility permission setup (now only the prompt on start and the tray status line)
- Linux: gnome (no kwin, no layer-shell), gnome-terminal, konsole sessions with a non-default profile; ghostty's `window-padding-balance`, splits stacked vertically and a bottom tab bar
- keys: wait up to 400 ms for candidates still being found; word start for wide chars and wrapped lines
- popup: pick by click is off; colors are kuterm's bundled themes, not the terminal's palette or font
- tests: pty byte-for-byte against no wrapper, ax fixtures
- tzc (and any plugin that draws its own menu) has to be off under compUI: `[[ -z $COMPUI_SOCKET ]] && source …`
- Windows
