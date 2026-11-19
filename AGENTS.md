# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

compUI: a fig-style completion popup for macOS terminals it doesn't own (ghostty first). Candidates come from [compcore](https://github.com/roworu/compcore) (pinned tag v0.1.3); compUI finds the cursor on screen, shows a native popup and puts the picked candidate into the shell line. `PLAN.md` is the design doc (target layout, protocols, phases) and its `status` section says what the current poc actually does versus the plan. Read it before structural changes.

## Commands

- `cargo install --path crates/compui` — the whole Linux install: one `compui` binary (wrapper, host and compcore linked from the pinned tag, zsh files embedded). Point the terminal's command at it; the first `compui` starts `compui host` in the background (own session, single instance via a lock), which exits 30s after the last wrapper closes. Config `~/.config/compui/config` (`enabled`, `debug`; optional, reread on change). Runtime files (zsh adapter, `host.log`, `host.lock`) in `$XDG_RUNTIME_DIR/compui` (else `~/.cache/compui`).
- `./build.sh` — builds `dist/compui`, and on macOS the Swift host `dist/compui-host` next to it (the wrapper starts it from there; it gets candidates from `compui serve`). Needs cargo, and swiftc on macOS.
- `./test.sh` — builds, kills a running host, then starts the wrapper in a pure zsh (temp empty `ZDOTDIR`) with `COMPUI_DEBUG=1`, so the host it starts logs everything. Interactive and manual: type `git ch`. Host log: `$XDG_RUNTIME_DIR/compui/host.log`.
- `cargo test` — tests in `compui-proto` (message round-trip) and `compui` (keys, reply and config parsing). Single test: `cargo test -p compui <name>`.
- The popup needs the macOS accessibility permission and a real terminal; there are no automated UI tests.

## Testing in Terminal.app

1. System Settings → Privacy & Security → Accessibility: allow Terminal. The host is started from it and inherits the permission.
2. In Terminal.app, run `./test.sh` (builds, restarts the host, opens a pure zsh under the wrapper).
3. In a second window or tab: `tail -f ~/.cache/compui/host.log`.
4. In the first one type `git ch`. The popup should sit under the word `ch`, one cell below the cursor.
5. Check up/down move the selection, tab inserts the candidate, esc hides it until the next word, and switching to another app hides it.
6. Repeat with a second tab, a resized window and a different font size; the popup must follow the cursor.
7. If it is off, send the `window … cell … origin` log line. A line without it means `AXBoundsForRange` worked (exact).

## Architecture

One binary, two roles, talking over unix sockets:

```
terminal ⇄ compui (wrapper) ⇄ zsh (shell/zsh/compui.zsh)
              └── /tmp/compui-<uid>.sock ──► compui host (linux, compcore linked)
                                             or compui-host (Swift, macos/common/main.swift; spawns `compui serve`)
```

- `crates/compui/src/pty.rs`: the wrapper, `compui` without arguments. Starts the host if the socket doesn't answer. Runs the shell in a pty and passes all bytes through untouched. Parses output with `vt100` to track the cursor cell and alt screen, owns a per-session socket for the zsh adapter, and takes up/down/tab/esc itself only while the host reports the popup `shown`. The wrapper is in every byte's path, so keep it thin (no engine, no UI).
- `shell/zsh/compui.zsh`: port of kuterm's completion adapter (protocol v1). Embedded in the binary and written to the runtime dir, injected via `ZDOTDIR`, reports each edit, applies `accept` lines with zle (dropped if `seq` is stale).
- `crates/compui-proto`: serde messages pty ⇄ host as JSON lines (`hello`, `line`, `key`, `shown`, `hidden`, `accept`, `end`), `"v":1`. The Swift host reimplements this by hand, so change both sides together.
- `macos/common/`: the Swift host (`main.swift`: sockets, compcore, tray, `NSPanel`) and the accessibility helpers plus the `Handler` protocol (`ax.swift`). Per-terminal placement lives in `macos/ghostty/` (padding read from its config, default 2), `macos/terminal/`, `macos/iterm2/` and `macos/generic/` (fallback); `handlers` in `main.swift` picks the first whose `matches` the frontmost bundle id. `build.sh` compiles them together with `swiftc`.

## Known divergences from PLAN.md

On macOS the host is Swift rather than the planned Rust `compui-host`; on Linux (KDE Wayland) it is Rust (`crates/compui/src/main.rs`, which pulls in `linux/common/main.rs`, `linux/ghostty/main.rs` and `linux/konsole/main.rs` via `#[path]`, popup via gtk4-layer-shell drawn with cairo/pango in `linux/common/popup.rs` (the Swift twin is `macos/common/popup.swift`, keep their look in sync), window position from a KWin script over D-Bus, compcore linked). The pty uses `vt100` rather than `alacritty_terminal`. The Rust `Handler` trait exists; on Linux there is no tray (the plan's `Tray` trait), the host is started on demand by the wrapper and configured by one file. Cargo workspace edition is 2024.
