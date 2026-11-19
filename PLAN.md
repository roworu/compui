# compUI plan

fig-style completion popup for terminals that compUI doesn't own: a native popup under the
typed word, in ghostty, terminal.app, iterm2 and later konsole, gnome-terminal and windows
terminal. candidates come from [compcore](https://github.com/roworu/compcore), which knows
nothing about ui; compUI finds the cursor on screen, shows the popup and puts the picked
candidate into the shell's line.

first target is macos only. linux and windows get placeholder handlers that report
"unsupported", so the popup stays off there and the terminal works as before.

## repos

| repo | used for |
| --- | --- |
| [roworu/compcore](https://github.com/roworu/compcore) | the engine, linked and bundled; protocol and specs in its [`docs/COMPLETIONS.md`](https://github.com/roworu/compcore/blob/main/docs/COMPLETIONS.md) |
| [roworu/kuterm](https://github.com/roworu/kuterm) | zsh adapter [`assets/shell/zsh/.zshenv`](https://github.com/roworu/kuterm/blob/main/assets/shell/zsh/.zshenv), protocol v1 [`src/completion/protocol.rs`](https://github.com/roworu/kuterm/blob/main/src/completion/protocol.rs), socket bridge [`src/completion/bridge.rs`](https://github.com/roworu/kuterm/blob/main/src/completion/bridge.rs), design notes [`docs/COMPLETIONS.md`](https://github.com/roworu/kuterm/blob/main/docs/COMPLETIONS.md) |
| [roworu/tzc](https://github.com/roworu/tzc) | compcore frontend drawn inside the terminal; not used by compUI, reference for menu behaviour |
| [withfig/autocomplete](https://github.com/withfig/autocomplete) | fig's spec format and ux, the reference |
| [microsoft/inshellisense](https://github.com/microsoft/inshellisense) | engine design compcore ported |
| [alacritty/alacritty](https://github.com/alacritty/alacritty) | `alacritty_terminal`, the screen parser in `compui-pty` ([zed's fork](https://github.com/zed-industries/alacritty) is what kuterm pins) |
| [ghostty-org/ghostty](https://github.com/ghostty-org/ghostty) | target terminal; config keys, ax behaviour |
| [gnachman/iTerm2](https://github.com/gnachman/iTerm2) | target terminal; ax behaviour |

## how fig did it, and what compUI uses instead

| fig | compUI |
| --- | --- |
| figterm: pty wrapper between terminal and shell | `compui-pty`, same idea: passes every byte through, parses the screen, takes popup keys |
| shell hooks reporting the edit buffer | [kuterm's zsh adapter](https://github.com/roworu/kuterm/blob/main/assets/shell/zsh/.zshenv), unchanged protocol v1 over a private socket per session |
| macos accessibility for window and cursor position | a handler per terminal (`macos/<terminal>/main.rs`), accessibility first |
| global key event tap | none: keys pass through `compui-pty` already, it takes up/down/tab/esc only while the popup shows |
| webview window | native `NSPanel` that never activates |

## pieces

```
terminal ⇄ compui-pty ⇄ zsh (compui.zsh adapter)
              │  pty pass-through, screen parser (cursor cell, grid, alt screen),
              │  popup keys, private socket for the adapter
              │
              └──unix socket──► compui-host (one per login session, CompUI.app)
                                  ├─ compcore engine, linked, specs loaded once
                                  ├─ handler for the terminal app → text area on screen
                                  └─ popup panel
```

| piece | job |
| --- | --- |
| compcore | candidates. linked as a crate into `compui-host`, pinned to a release tag. not changed by compUI |
| `compui-pty` | started by the terminal instead of the shell. runs the shell in a pty, forwards input, output, resizes, signals and exit code untouched. parses output with `alacritty_terminal` (as kuterm does) to know the cursor cell, rows and cols, application cursor mode and the alt screen. owns the adapter's socket. stays thin: no engine, no ui |
| `compui.zsh` | kuterm's [`assets/shell/zsh/.zshenv`](https://github.com/roworu/kuterm/blob/main/assets/shell/zsh/.zshenv) completion adapter, with `KUTERM_COMPLETION_SOCKET` renamed to `COMPUI_SOCKET`. reports every edit, applies `accept` lines with zle |
| `compui-host` | one process per user session, needs the accessibility permission (so it lives in a signed `.app`). runs compcore, picks the handler for the terminal app, places and draws the popup, hides it on focus/move/resize. stays in the system tray while running, see tray |
| handlers | one folder per os and terminal, see layout. each turns "this app, this grid" into a text area rect on screen |

the engine sits in the host, not in each `compui-pty`: specs and the help probe cache are
loaded once, and the wrapper, which every byte goes through, stays small.

## layout

```
compui/
  Cargo.toml                    workspace
  PLAN.md
  crates/
    compui-proto/               messages pty ⇄ host, json lines, serde
    compui-pty/                 the wrapper binary
    compui-host/                host binary: socket server, engine, view model, placement,
                                handler registry (cfg'd per os, see below)
  macos/
    common/main.rs              ax helpers, AXObserver hide triggers, NSPanel popup, NSStatusItem tray
    generic/main.rs             unknown terminals: window frame + ws_xpixel, best effort
    ghostty/main.rs
    terminal/main.rs            terminal.app
    iterm2/main.rs
  linux/
    common/main.rs              tray (StatusNotifierItem), placeholder
    konsole/main.rs             placeholder
    gnome-terminal/main.rs      placeholder
    ghostty/main.rs             placeholder
  windows/
    common/main.rs              tray (Shell_NotifyIcon), placeholder
    windows-terminal/main.rs    placeholder
  shell/
    zsh/compui.zsh
  packaging/
    macos/                      Info.plist, launch agent plist, build/sign/notarize script
```

each handler folder is a module whose entry is `main.rs`; `compui-host` pulls them in with
`#[cfg(target_os = "...")] #[path = "../../../macos/ghostty/main.rs"] mod ghostty;`. extra
files of a handler sit next to its `main.rs`.

handler interface, in `compui-host`:

```rust
/// a terminal app the popup can be placed over
pub trait Handler {
    /// true when this handler knows the app (bundle id on macos, exe name elsewhere)
    fn matches(&self, app: &App) -> bool;
    /// screen rect of the cell grid of the focused session, in points, top-left origin;
    /// None when it can't be known (split it can't see, permission missing, ...)
    fn locate(&self, app: &App, grid: &Grid) -> Option<TextArea>;
}
```

placeholders implement `matches` and return `None` from `locate`. drawing the popup and the tray icon
are per os (`<os>/common`), not per terminal.

## tray

`compui-host` is a tray app on every platform: no dock icon or taskbar entry, one tray icon for
as long as it runs, so the user can see it is alive and quit it. the menu has: a status line
(running, or why the popup is off: permission missing, unsupported terminal or os), enable /
disable popup, quit. the host exposes it through a small `Tray` trait in `compui-host`, one
implementation per os in `<os>/common`:

| os | tray | no dock / taskbar entry |
| --- | --- | --- |
| macos | `NSStatusItem` | `LSUIElement = true` in `Info.plist` (`NSApplicationActivationPolicyAccessory`) |
| linux | StatusNotifierItem over d-bus; on gnome needs the appindicator extension | no window |
| windows | `Shell_NotifyIcon` | no window |

on linux and windows the tray comes up with the placeholder handlers, showing "unsupported".
quit stops the host; the launch agent does not restart it after a quit from the menu.

## protocols

zsh ⇄ `compui-pty`: kuterm's completion protocol v1 ([`src/completion/protocol.rs`](https://github.com/roworu/kuterm/blob/main/src/completion/protocol.rs)) unchanged (`hello`, `env`, `line`, `end`
from the shell; `1 accept <seq> <start> <end> <text>` back). see kuterm's
[`docs/COMPLETIONS.md`](https://github.com/roworu/kuterm/blob/main/docs/COMPLETIONS.md#protocol-version-1).

`compui-pty` ⇄ `compui-host`, json lines, `"v":1`:

```
→ {"type":"hello","session":<pty pid>,"shell_pid":…,"tty":"/dev/ttys004"}
→ {"type":"env","session":…,"path":"<PATH>"}
→ {"type":"line","session":…,"seq":7,"buffer":"git ch","cursor":6,"cwd":"/abs",
   "cell":{"row":12,"col":8},"grid":{"rows":40,"cols":120,"xpx":1440,"ypx":960},"history":false}
→ {"type":"key","session":…,"seq":7,"key":"up"|"down"|"accept"|"dismiss"}
→ {"type":"end","session":…}
← {"type":"shown","session":…,"seq":7}          popup visible, wrapper starts taking keys
← {"type":"hidden","session":…}                 wrapper stops taking keys
← {"type":"accept","session":…,"seq":7,"start":4,"end":6,"text":"checkout"}
```

`cell` is the cursor cell from the parser; the host gets the word's first cell by stepping
back the display width of `buffer[start..cursor]`, wrapping at `cols`. `xpx/ypx` are the
pty's pixel size when the terminal reports one (ghostty does), else 0.

the wrapper forwards `accept` to zsh as the v1 `accept` line; zsh drops it if `seq` is not the
line's current one.

## keys

while the host says `shown`, `compui-pty` takes, instead of forwarding:

- up / down: `\e[A` `\e[B`, or `\eOA` `\eOB` in application cursor mode
- tab: accept
- esc: a lone `\e` with nothing after it in the same read; dismiss until the next word

everything else is forwarded and the line change hides or refreshes the popup. a key that
arrives while candidates are still being found waits for them, at most 400 ms, then goes to
the shell (same rule as kuterm). never taken on the alt screen.

## macos

**handler order**: the handler matching the app's bundle id, else `generic`. a handler
returning `None` means no popup for that line.

**finding the app**: walk parents of `shell_pid` (`proc_pidinfo`) up to a process with a
bundle id; `AXUIElementCreateApplication(pid)` → `AXFocusedWindow`. a line only arrives on a
keystroke, so the focused window is the one typed in.

**per terminal**, to confirm in phase 0:

| terminal | bundle id | first choice | fallback |
| --- | --- | --- | --- |
| terminal.app | `com.apple.Terminal` | `AXTextArea` of the selected tab, `AXBoundsForRange` of the cursor | window frame + cell size |
| iterm2 | `com.googlecode.iterm2` | `AXTextArea` of the focused session (per split), `AXBoundsForRange` | window frame + cell size |
| ghostty | `com.mitchellh.ghostty` | ax text element of the focused surface if it has one | window frame + `xpx/ypx` cell size + `window-padding-*` read from ghostty's config |
| other | any | — | `generic`: window frame + `xpx/ypx`, only when the grid fills the window |

**split check** for fallbacks: if the text area divided by the cell size is not `cols × rows`,
the session is in a split or the guess is off; return `None`.

**coordinates**: ax gives points with a top-left origin on the main display; `NSPanel` frames
use bottom-left. convert once in `macos/common`. points, so retina needs nothing extra.

**popup**: `NSPanel` with `.nonactivatingPanel | .borderless`, level `.popUpMenu`,
`hidesOnDeactivate = false`, collection behavior `.canJoinAllSpaces | .fullScreenAuxiliary`,
ignores mouse in v1 (a click would activate it). under the first char of the word, flipped
above when there's no room, clamped to the screen's `visibleFrame`. kind colors as in kuterm:
subcommands magenta, suggestions cyan, options yellow, commands green, folders blue, files in
the text color, taken from a default palette in v1.

**hide** on `kAXFocusedWindowChangedNotification`, `kAXWindowMovedNotification`,
`kAXWindowResizedNotification` of the terminal app, on app switch
(`NSWorkspace.didActivateApplicationNotification`), on `end`, and when the wrapper sees the
alt screen.

**permission**: `AXIsProcessTrustedWithOptions` with the prompt on first start; without it
every `locate` returns `None` and the tray status line says why.

## package

macos: `CompUI.app`, universal (aarch64 + x86_64 via `lipo`), signed and notarized, since the
accessibility permission is tied to the signature.

```
CompUI.app/Contents/
  MacOS/compui-host
  MacOS/compui-pty
  MacOS/compcore              the engine's cli from the same pinned tag, for `compcore complete`
  Resources/shell/zsh/compui.zsh
  Resources/io.github.roworu.compui.plist   launch agent, starts the host at login
```

compcore is carried twice from one pinned tag: linked into `compui-host`
(`compcore = { git = "https://github.com/roworu/compcore", tag = "v0.1.3" }`, specs embedded),
and its cli next to it for debugging. `compui-host --version` prints both versions.

setup for the user: point the terminal's command at `compui-pty` (ghostty `command =`,
terminal.app profile "run command", iterm2 profile command), or add to `.zshrc`:

```zsh
[[ -z $COMPUI_SOCKET ]] && exec /Applications/CompUI.app/Contents/MacOS/compui-pty
```

`compui-pty` starts the user's login shell, sets `ZDOTDIR` the way kuterm's injection does to
source `compui.zsh`, then the user's own rc files.

## phases

| # | phase | done when |
| --- | --- | --- |
| 0 | spike: inspect terminal.app, iterm2, ghostty with accessibility inspector (text element, `AXBoundsForRange`, per split/tab), check ghostty's `xpx/ypx` and padding, test an `NSPanel` over full screen spaces without stealing focus, measure pass-through latency of a bare pty wrapper | a table per terminal: exact, fallback or none |
| 1 | workspace, `compui-proto`, `compui-pty` pass-through with parser, `compui.zsh` ported from kuterm | zsh runs under the wrapper with no visible change; vim, less, tmux, mouse, bracketed paste, resize, ctrl-c, ctrl-z and exit codes behave |
| 2 | `compui-host`: socket, compcore, tray icon, panel at a fixed spot, keys and accept end to end | completing works in any terminal, popup in the wrong place |
| 3 | `macos/terminal` | popup under the word in terminal.app, tabs |
| 4 | `macos/iterm2` | same, with splits |
| 5 | `macos/ghostty` | same in ghostty; splits via ax or `None` |
| 6 | `macos/generic`, hide triggers, multiple screens, spaces, full screen | — |
| 7 | packaging: `.app`, universal, sign, notarize, launch agent, permission flow | installs and runs on a clean mac |
| 8 | linux and windows handlers, see below | — |

## status

a working macos poc, tested by hand in ghostty with zsh. it differs from the layout above:
the host is swift (`macos/common/` plus one folder per terminal, a swift `Handler` protocol), not rust in `compui-host`, because it was
built and checked without a rust toolchain first; `compui-pty` parses the screen with `vt100`
instead of `alacritty_terminal`. `./build.sh` builds `dist/`, `./test.sh` builds and starts a pure
zsh (empty `ZDOTDIR`) under the wrapper.

done:

- `compui-proto`: `hello`, `line` (buffer, cursor, cwd, path, cell, grid), `key`, `shown`, `hidden`,
  `accept`, `end`; round-trip test
- `compui-pty`: pty pass-through with exit code, size polling, `vt100` cursor cell (seeded with a cursor position query at start), adapter socket
  per session, line sent once the screen settles, popup keys (up, down, tab, esc) taken while shown,
  accept forwarded to zsh, `end` on alt screen
- `shell/zsh`: kuterm's completion adapter ported, injected through `ZDOTDIR`
- host: socket server, `compcore serve` (v0.1.3, built from the pinned tag into `dist/`), popup
  `NSPanel` with kind colors and selection, tray item with quit, hide on app switch, window move, resize and focus change (`AXObserver`), history
  navigation, empty line, alt screen and a window that is not the session's terminal (parents of the
  pty pid vs the frontmost app); no popup again until the next word after accept or esc
- placement: exact `AXBoundsForRange` where a terminal gives it, else cell size from the pty's pixel
  size and the ax text area's origin (ghostty: surface frame plus its padding of 2)
- launcher `dist/compui` starts the host, and replaces one whose socket is dead

left:

- phase 2: move the host to rust (`compui-host`, compcore linked, handler trait, `Tray` trait), or
  decide to keep swift for macos; `compui-pty` on `alacritty_terminal`
- phase 4: `macos/iterm2` is written from the plan (focused element, exact path only) and untested, no iterm2 on the dev machine
- phase 5: split check (`cols × rows`)
- phase 6: multiple screens, spaces, full screen
- keys: wait up to 400 ms for candidates still being found; word start for wide chars and wrapped lines
- popup: pick by mouse is off; kind colors are fixed, not from the terminal palette
- permission flow: the prompt shows on start, and the tray status line says when it is missing; no guided setup yet
- phase 7: `.app`, universal, sign, notarize, launch agent
- phase 8: linux and windows handlers and trays
- tests: pty byte-for-byte comparison against no wrapper, placement math, ax fixtures (key filter has a unit test)
- tzc (and any plugin that draws its own menu) has to be off under compUI, `[[ -z $COMPUI_SOCKET ]] && source …`

## later: linux and windows

placeholders until macos is done. what goes in them, from the earlier research:

| handler | locate via | popup via |
| --- | --- | --- |
| `linux/konsole` | kwin script over d-bus (active window, pid, geometry), at-spi if konsole gives character extents | layer-shell surface (kwin supports it) |
| `linux/gnome-terminal` | gnome shell extension (focus window, pid, frame rect), at-spi text of vte | the extension draws it, mutter has no layer-shell |
| `linux/ghostty` | kwin script or gnome extension by desktop; `xpx/ypx` + config padding | as above by desktop |
| `windows/windows-terminal` | ui automation `TextPattern` caret bounds, per pane | topmost `WS_EX_NOACTIVATE` window |

`compui-pty` needs a conpty backend on windows; shells there need a pwsh adapter
(psreadline). zsh in wsl talks to a windows host through `compui-host.exe` run over interop.

## tests

- `compui-proto`, placement math, key filter, wrap math: unit tests, any os.
- `compui-pty`: run shells and full screen programs under it in a pty, compare output byte for
  byte with no wrapper.
- handlers: ax trees recorded in phase 0 as fixtures; live checks are manual, ci runners can't
  grant the accessibility permission.

## risks

- the wrapper is in every byte's path: any bug breaks the terminal. keep it small, fuzz the
  parser input path, fall back to plain pass-through on any internal error.
- ghostty on macos may expose no ax text element: then splits get no popup.
- fallback math depends on padding and title/tab bar height; off by a few points if a
  terminal's config can't be read.
- ssh and containers: the wrapper sees the remote screen but no adapter talks to it; no popup
  there in v1.
