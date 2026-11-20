# windows support: implementation plan (compui side)

windows terminal and the classic console (conhost); cmd, windows powershell and pwsh. compcore's part
(`\` paths, `PATHEXT`, cmd/pwsh quoting, builtins) is done separately; compui bumps the pin when it is tagged.

## phase 0: spikes (scratch, not committed)

1. uia cursor bounds: in windows terminal (one tab, split panes, 125/150% scale) and conhost, check
   `FocusedElement → TextPattern.GetSelection()[0] → ExpandToEnclosingUnit(Character) → GetBoundingRectangles()`
   gives the cursor cell in physical px.
2. nested conpty: a 20-line rust wrapper around `cmd.exe` through `portable-pty`; check input latency, resize,
   ctrl+c, cmd's line editing (history, F7, esc clears the line).
3. osc 133 A/B/C and 9;9 survive the inner conpty's re-rendering.

if 1 fails in windows terminal, placement falls back to window rect + `settings.json` (see risks).

## phase 1: portability (wrapper and shared code build on windows)

| where | change |
|---|---|
| `crates/compui/Cargo.toml` | `libc` under `cfg(unix)`; `windows` crate under `cfg(windows)` (Console, Pipes, Threading, ToolHelp, Com, Accessibility, WindowsAndMessaging, Gdi, Dwm, HiDpi, Direct2D, DirectWrite, Registry) |
| `crates/compui/src/ipc.rs` (new) | `Listener`/`Stream` aliases: `UnixListener`/`UnixStream` on unix, named pipes on windows (`\\.\pipe\compui-<user>`, `PIPE_REJECT_REMOTE_CLIENTS`); used by `pty.rs` and `main.rs` (`serve`, `reply`, `Host.clients`, `Ev::Client`) |
| `crates/compui/src/term.rs` (new) | `term_size`, `raw_mode`/restore, `cursor_start` moved out of `pty.rs`; windows: `GetConsoleScreenBufferInfo`, `SetConsoleMode` (vt input on, line/echo/processed input off on stdin; vt processing on stdout), `WaitForSingleObject` on stdin instead of `poll` |
| `pty.rs::connect_host` | windows: `DETACHED_PROCESS \| CREATE_NO_WINDOW \| CREATE_NEW_PROCESS_GROUP` instead of `setsid` |
| `main.rs::runtime_dir`, `Config::path`, `specs()` | windows: `%LOCALAPPDATA%\compui`, `%APPDATA%\compui\config`, `%LOCALAPPDATA%\compcore` |
| `main.rs::ppid` | windows: `CreateToolhelp32Snapshot`; `is_ancestor` unchanged (wt is the wrapper's parent; a conhost window reports its client's pid) |
| `main.rs::host` | `cfg(windows)` branch calling `win::run(handlers)`; `File::try_lock` already works |
| `build.rs` | nothing, it returns early off macos |

the zsh path (`write_zsh`, adapter socket, `ZDOTDIR`) goes under `cfg(unix)`.

done when: `cargo build --target x86_64-pc-windows-msvc` and `cargo test` pass on linux and a windows runner,
no behavior change on unix.

## phase 2: wrapper, screen-scraping adapter

no shell adapter connection on windows: the shell only marks its prompt, the wrapper reads the line off the screen.

### 2a. shell start (`pty.rs`)

- `compui [pwsh|powershell|cmd]`, default: pwsh, else powershell, else cmd on `PATH`. no config key, so
  `main.swift` doesn't change.
- cmd: set `PROMPT` in the child env to `$e]133;D$e\$e]133;A$e\$e]9;9;$P$e\` + (user's `PROMPT` or `$P$G`)
  + `$e]133;B$e\`.
- pwsh/powershell: `-NoExit -Command ". '<runtime>\compui.ps1'"`; it runs after the user's profile, so
  starship/oh-my-posh prompts are already defined.
- `shell/windows/compui.ps1` (new, embedded, written out like the zsh files): wraps the existing `prompt` with
  133 A/B and 9;9; `Set-PSReadLineOption -PredictionSource None` (gray inline text would be read as buffer,
  same idea as the tzc note); binds `Ctrl+Shift+F12` to `RevertLine` so accept doesn't depend on edit mode
  (emacs mode's esc is a prefix).

### 2b. screen text (`cursor.rs`)

- parse osc 133 A/B/C/D and 9;9; keep the B mark (row, col), shifted on scroll.
- a char-only grid (no attributes) updated only while a B mark is open (after B, before C): print (wide chars),
  EL, ED, ECH, ICH, DCH, scroll, resize (drop the mark if unsure). the unix fast path is unchanged, the grid is
  off unless scraping.
- tests against `vt100` on recorded conpty output (cmd typing, history recall, pwsh with syntax colors,
  a wrapped line, cjk).

### 2c. line reports (`pty.rs`, scrape mode)

- after output settles (existing QUIET/SETTLE thread), with a B mark open: buffer = cells from B to the last
  non-blank cell of the input rows (wrapped rows joined, trailing spaces trimmed); cursor = the cursor cell's
  char index in it. if (buffer, cursor) changed: `seq += 1`, send `Line` (`cwd` from 9;9, `path` from the
  wrapper's `PATH`, `history: false`).
- `recent`: an in-memory list per session; on enter with a B mark open (or 133;C) push the buffer; send entries
  starting with the text before the cursor, newest first. pwsh: also seed from the tail of PSReadLine's
  `HistorySavePath` (`compui.ps1` prints its path once in a private osc the wrapper eats).
- `End` on 133;C, alt screen and exit, as now.

### 2d. accept (`talk` in `pty.rs`)

- offsets are char offsets, as with zsh's `$BUFFER`.
- if `seq` is current: new line = `buf[..start] + text + buf[end..]`, cursor = `start + len(text)`. write to the
  shell's input: clear (`\x1b` for cmd, `\x1b[24;6~` for pwsh), the new line, then `\x1b[D` × chars after the
  cursor. done under the state lock so typed-ahead keys can't interleave. stale seq: dropped, like zsh.

done when: `compui cmd` / `compui pwsh` behave like the bare shell in windows terminal with no host, and
`COMPUI_DEBUG=1` logs the right buffer, cursor and cell for each key.

## phase 3: windows host

new files mirroring `linux/`, pulled into `main.rs` with `#[path]` under `cfg(windows)`; the shared
`Host<P>`, `Handler` and `Popup` are reused unchanged.

| file | what |
|---|---|
| `windows/common/main.rs` | `run(handlers)`: per-monitor dpi aware v2, a message-only window, a thread draining the `async_channel` and posting `WM_APP` so `Host::on` runs on the ui thread. `SetWinEventHook` (`EVENT_SYSTEM_FOREGROUND`, `EVENT_OBJECT_LOCATIONCHANGE` for the foreground hwnd) → `Ev::Window(App)` from `GetWindowThreadProcessId`, `QueryFullProcessImageNameW` (exe stem, e.g. `WindowsTerminal`, so `terminal.<name> = off` works), `DWMWA_EXTENDED_FRAME_BOUNDS`, `MonitorFromWindow`, `GetDpiForWindow / 96` |
| `windows/common/uia.rs` | a dedicated mta thread owning `IUIAutomation` (no uia client calls from a thread with windows). `cursor_rect() -> Option<Rect>`: focused element → `TextPattern` → selection range (the cursor when nothing is selected) → expand to character → bounds. the ui thread waits ≤ ~50 ms |
| `windows/common/popup.rs` | `Popup` impl: hidden `WS_EX_TOPMOST \| WS_EX_NOACTIVATE \| WS_EX_TOOLWINDOW \| WS_EX_LAYERED` window; `show` runs `arrange`/`draw` (a direct2d/directwrite twin of `linux/common/popup.rs`, same constants and themes, kept in sync like `popup.swift`) into a premultiplied dib → `UpdateLayeredWindow`. `WM_MOUSEACTIVATE` → `MA_NOACTIVATE`, `WM_MOUSEWHEEL` → `Ev::Scroll`. dark from the `AppsUseLightTheme` registry value |
| `windows/terminal/main.rs` | `Handler` for `WindowsTerminal`: uia cursor rect, shifted to the word start |
| `windows/conhost/main.rs` | `Handler` for the classic console: uia, else `GetGUIThreadInfo().rcCaret` |

- word start: `Host::on_cands` already turns the cursor cell into the word's start col; the uia handlers also
  need the cursor col, so add `cursor_col` to the host's internal `Grid` (not the protocol). rect = cursor rect
  − (cursor_col − col) × cell width (from the uia rect).
- idle exit and hiding on app switch: already in `Host` (`Ev::Gone`/`Ev::Idle`, `Ev::Window`).
- protocol: no change; swift untouched.

done when: the "testing in terminal.app" checklist from `CLAUDE.md` passes in windows terminal (tabs, split
panes, resize, font size, 100/150% scale, two monitors with different dpi) and in conhost.

## phase 4: build, release, install

- `build.sh`: copy `compui.exe` when present (github windows runners have bash).
- `release.yml`: `{ runner: windows-latest, name: windows-x86_64 }`, `{ runner: windows-11-arm, name: windows-arm64 }`;
  same `tar.gz` (windows 10+ has `tar.exe`).
- `install.ps1` (new): download into `%LOCALAPPDATA%\Programs\compui`; write a windows terminal fragment
  `%LOCALAPPDATA%\Microsoft\Windows Terminal\Fragments\compui\compui.json` with "pwsh (compUI)",
  "Windows PowerShell (compUI)", "cmd (compUI)" for the shells that exist; `settings.json` untouched.
  `-Uninstall`: stop the host, remove the exe, the fragment, `%LOCALAPPDATA%\compui`, `%APPDATA%\compui`,
  `%LOCALAPPDATA%\compcore`. kept in sync with what compui writes, like `install.sh`.
- `test.ps1` (new): the windows `test.sh` (build, kill the host, `compui pwsh -NoProfile` with `COMPUI_DEBUG=1`).

## phase 5: docs

`CLAUDE.md`/`README`: a windows section (layout, scraping, the checklist); drop "Windows" from "not done yet".

## risks and fallbacks

- uia gives no cursor bounds in windows terminal: window rect + `settings.json` (padding, font, tab bar),
  logged like the macos `window … cell … origin` line.
- windows terminal started through the default-terminal handoff (`is_ancestor` fails): the wrapper reports
  `GetAncestor(GetConsoleWindow(), GA_ROOTOWNER)` in an optional `window` field of `Line` (`serde(default)`;
  swift ignores it).
- prompts that redraw the line themselves (transient, right prompts): limit the buffer to the B row's width,
  drop right-prompt text past a gap. later: a PSReadLine predictor or clink adapter for exact buffers.
- retyping on accept can fire PSReadLine/clink handlers bound to typed chars; fine at first, an exact adapter
  fixes it.
- out of scope: wsl shells, other windows terminals (wezterm, alacritty), click to pick.

## order

0 → 1 → 2 (testable with no host) → 3 → 4 → 5, one commit/pr each; the compcore pin bump lands whenever its
windows tag is ready.
