//! `compui`: runs the shell wrapper (`pty.rs`) and starts the host if none answers. `compui host`
//! is the host: listens on /tmp/compui-<uid>.sock for wrapper messages, asks compcore (linked) for
//! candidates, finds the cursor of the active terminal through a per-terminal handler and shows a popup
//! under the typed word. the rust host is linux only; macos runs the swift host in `macos/`, which
//! gets candidates from `compui serve`
#![cfg_attr(not(target_os = "linux"), allow(dead_code, unused))]

mod cursor;
mod pty;

#[cfg(target_os = "linux")]
#[path = "../../../linux/common/main.rs"]
mod common;
#[cfg(target_os = "linux")]
#[path = "../../../linux/ghostty/main.rs"]
mod ghostty;
#[cfg(target_os = "linux")]
#[path = "../../../linux/konsole/main.rs"]
mod konsole;

use compui_proto::Msg;
use std::collections::HashMap;
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::io::PipeWriter;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::SystemTime;
use std::{env, fs, thread};

/// `debug` from the config, or COMPUI_DEBUG set
static DEBUG: AtomicBool = AtomicBool::new(false);

/// prints to stderr (the log file, see `runtime_dir`) when debug is on
pub fn log(m: &str) {
    if DEBUG.load(Ordering::Relaxed) || env::var_os("COMPUI_DEBUG").is_some() {
        eprintln!("{m}");
    }
}

/// `~/.config/compui/config`: `key = value` lines, `#` comments
#[derive(Debug, PartialEq)]
pub struct Config {
    /// show the popup at all
    pub enabled: bool,
    /// log every line, window and placement
    pub debug: bool,
}

impl Config {
    /// parses the config text, defaults for what is missing or unreadable
    pub fn parse(text: &str) -> Self {
        let mut c = Config { enabled: true, debug: false };
        for line in text.lines() {
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.split('#').next().unwrap_or_default().trim();
            let on = matches!(v, "true" | "yes" | "on" | "1");
            match k.trim() {
                "enabled" => c.enabled = on,
                "debug" => c.debug = on,
                _ => {}
            }
        }
        c
    }

    pub fn path() -> PathBuf {
        let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
        env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config")).join("compui/config")
    }
}

/// per-user dir for the zsh files, the host's log and lock: $XDG_RUNTIME_DIR/compui, else ~/.cache/compui
pub fn runtime_dir() -> PathBuf {
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from).unwrap_or_else(|| home.join(".cache")).join("compui")
}

/// compcore's specs, with probed help cached in `$XDG_CACHE_HOME/compcore` like its own binary does
fn specs() -> compcore::Specs {
    let cache = env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .filter(|d| d.is_absolute())
        .or_else(|| env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
        .map(|d| d.join("compcore"));
    compcore::Specs::load(None, cache)
}

/// a rect in logical pixels, global compositor coordinates, top-left origin
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Rect {
    pub x: f64,
    pub y: f64,
    pub w: f64,
    pub h: f64,
}

/// the active window as the compositor reports it
#[derive(Clone, Debug, PartialEq, Default)]
pub struct App {
    pub pid: u32,
    /// process name from /proc, like `ghostty`
    pub exe: String,
    /// the window's client area, without server side decorations
    pub rect: Rect,
    /// output (monitor connector) the window is on
    pub output: String,
    pub scale: f64,
}

/// the word's first cell and the terminal grid, as reported by compui-pty
pub struct Grid {
    pub row: u16,
    pub col: u16,
    pub rows: u16,
    pub cols: u16,
    pub xpx: u16,
    pub ypx: u16,
}

/// a terminal app the popup can be placed over
pub trait Handler {
    /// true when this handler knows the app
    fn matches(&self, app: &App) -> bool;
    /// screen rect of the word's first cell, or None when it can't be known
    fn locate(&self, app: &App, grid: &Grid) -> Option<Rect>;
}

pub struct Cand {
    pub label: String,
    pub insert: String,
    pub desc: String,
    pub kind: String,
}

/// events for the ui thread
pub enum Ev {
    Client(u64, UnixStream),
    Msg(u64, Msg),
    Gone(u64),
    /// one compcore reply record
    Cands(String),
    /// the active window changed, moved or resized
    Window(App),
    /// some time after the last wrapper left
    Idle,
    Quit,
}

/// candidates shown at once, the list scrolls past them
pub const ROWS: usize = 8;

/// what the platform side draws
pub trait Popup {
    /// shows `cands[top..top + ROWS]` with `selected` highlighted under `cell`, on `app`'s output
    fn show(&mut self, app: &App, cell: Rect, cands: &[Cand], selected: usize, top: usize);
    fn hide(&mut self);
}

struct Ctx {
    client: u64,
    session: u32,
    seq: u64,
    row: u16,
    col: u16,
    rows: u16,
    cols: u16,
    xpx: u16,
    ypx: u16,
    cursor: usize,
}

pub struct Host<P: Popup> {
    pub popup: P,
    pub handlers: Vec<Box<dyn Handler>>,
    config: Config,
    /// mtime of the config file when it was read, to reread it when it changes
    config_mtime: Option<SystemTime>,
    clients: HashMap<u64, UnixStream>,
    compcore: PipeWriter,
    tx: async_channel::Sender<Ev>,
    app: App,
    cands: Vec<Cand>,
    selected: usize,
    /// first candidate shown
    top: usize,
    ctx: Option<Ctx>,
    word: (usize, usize),
    /// word the user accepted or dismissed, no popup until the next one
    suppressed: Option<(u64, usize)>,
    shown: Option<u64>,
    latest: u64,
    request: Option<(u64, Ctx)>,
}

fn reply(s: &mut UnixStream, m: &Msg) {
    let _ = s.write_all((serde_json::to_string(m).unwrap() + "\n").as_bytes());
}

/// parent pid of `pid` from /proc
fn ppid(pid: u32) -> Option<u32> {
    let stat = fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    // the name in parens may hold spaces, so split after the last ')'
    stat.rsplit_once(')')?.1.split_whitespace().nth(1)?.parse().ok()
}

/// true when `pid` is `of` or one of its parents
fn is_ancestor(pid: u32, of: u32) -> bool {
    let mut p = of;
    for _ in 0..64 {
        if p == pid {
            return true;
        }
        match ppid(p) {
            Some(q) if q > 1 => p = q,
            _ => return false,
        }
    }
    false
}

/// `serve` reply: seq, start, end, then candidates
fn parse_reply(rec: &str) -> Option<(u64, usize, usize, Vec<Cand>)> {
    let mut parts = rec.split('\u{1e}');
    let head: Vec<&str> = parts.next()?.split('\u{1f}').collect();
    let [id, s, e] = head[..] else { return None };
    let cands = parts
        .filter(|p| !p.is_empty())
        .map(|p| {
            let f: Vec<&str> = p.split('\u{1f}').collect();
            let get = |i: usize| f.get(i).copied().unwrap_or_default().to_string();
            Cand { label: get(0), insert: get(1), desc: get(2), kind: get(3) }
        })
        .collect();
    Some((id.parse().ok()?, s.parse().ok()?, e.parse().ok()?, cands))
}

impl<P: Popup> Host<P> {
    /// runs compcore's serve loop on a thread over pipes; its replies go to `tx`
    pub fn new(popup: P, handlers: Vec<Box<dyn Handler>>, tx: async_channel::Sender<Ev>) -> Self {
        let (req_r, req_w) = std::io::pipe().expect("pipe");
        let (mut out, rep_w) = std::io::pipe().expect("pipe");
        thread::spawn(move || compcore::serve::run(BufReader::new(req_r), rep_w, &specs(), true));
        let tx2 = tx.clone();
        thread::spawn(move || {
            let (mut pending, mut buf) = (Vec::new(), [0u8; 8192]);
            while let Ok(n) = out.read(&mut buf) {
                if n == 0 {
                    break;
                }
                pending.extend_from_slice(&buf[..n]);
                while let Some(z) = pending.iter().position(|b| *b == 0) {
                    let rec = String::from_utf8_lossy(&pending[..z]).into_owned();
                    pending.drain(..=z);
                    let _ = tx.send_blocking(Ev::Cands(rec));
                }
            }
        });
        Host {
            popup,
            handlers,
            config: Config::parse(""),
            config_mtime: None,
            clients: HashMap::new(),
            compcore: req_w,
            tx: tx2,
            app: App::default(),
            cands: Vec::new(),
            selected: 0,
            top: 0,
            ctx: None,
            word: (0, 0),
            suppressed: None,
            shown: None,
            latest: 0,
            request: None,
        }
    }

    /// rereads the config when its file changed, so edits apply on the next keystroke without a restart
    pub fn reload(&mut self) {
        let path = Config::path();
        let mtime = fs::metadata(&path).and_then(|m| m.modified()).ok();
        if mtime == self.config_mtime && mtime.is_some() {
            return;
        }
        self.config_mtime = mtime;
        let c = Config::parse(&fs::read_to_string(&path).unwrap_or_default());
        DEBUG.store(c.debug, Ordering::Relaxed);
        if c != self.config {
            eprintln!("config {}: {c:?}", path.display());
            self.config = c;
        }
    }

    pub fn hide(&mut self) {
        self.popup.hide();
        if let (Some(id), Some(c)) = (self.shown.take(), &self.ctx)
            && let Some(s) = self.clients.get_mut(&id)
        {
            reply(s, &Msg::Hidden { v: 1, session: c.session });
        }
    }

    fn hide_if(&mut self, client: u64) {
        if self.shown == Some(client) {
            self.hide();
        }
    }

    fn show(&mut self) {
        let Some(c) = &self.ctx else { return };
        let grid = Grid { row: c.row, col: c.col, rows: c.rows, cols: c.cols, xpx: c.xpx, ypx: c.ypx };
        let Some(cell) = self.handlers.iter().find(|h| h.matches(&self.app)).and_then(|h| h.locate(&self.app, &grid)) else {
            log(&format!("no cursor rect for {:?}", self.app));
            return self.hide();
        };
        self.popup.show(&self.app, cell, &self.cands, self.selected, self.top);
        if self.shown.is_none() {
            self.shown = Some(c.client);
            if let Some(s) = self.clients.get_mut(&c.client) {
                reply(s, &Msg::Shown { v: 1, session: c.session });
            }
        }
    }

    fn on_cands(&mut self, rec: &str) {
        let Some((id, start, end, cands)) = parse_reply(rec) else { return };
        log(&format!("candidates {}", cands.len()));
        if self.request.as_ref().map(|r| r.0) != Some(id) {
            return;
        }
        let (_, c) = self.request.take().unwrap();
        let client = c.client;
        // shortcut: the word's first cell is the cursor cell minus its chars, no wrapping or wide chars
        let col = c.col.saturating_sub(c.cursor.saturating_sub(start) as u16);
        self.ctx = Some(Ctx { col, ..c });
        if cands.is_empty() || self.suppressed == Some((client, start)) {
            return self.hide_if(client);
        }
        self.cands = cands;
        self.word = (start, end);
        self.selected = 0;
        self.top = 0;
        self.show();
    }

    fn on_msg(&mut self, client: u64, m: Msg) {
        match m {
            Msg::End { .. } => {
                self.suppressed = None;
                self.request = None;
                self.hide_if(client);
            }
            Msg::Line { session, seq, buffer, cursor, cwd, path, history, cell, grid, .. } => {
                let active = self.app.pid != 0 && is_ancestor(self.app.pid, session);
                self.reload();
                log(&format!("line {buffer:?} cell {cell:?} grid {grid:?} active {active} enabled {}", self.config.enabled));
                self.latest += 1;
                self.request = None;
                if history || !active || !self.config.enabled || buffer.trim().is_empty() {
                    return self.hide_if(client);
                }
                let ctx = Ctx { client, session, seq, row: cell.row, col: cell.col, rows: grid.rows, cols: grid.cols, xpx: grid.xpx, ypx: grid.ypx, cursor };
                self.request = Some((self.latest, ctx));
                let req = format!("{}\u{1f}{cursor}\u{1f}{cwd}\u{1f}{path}\u{1f}{buffer}\0", self.latest);
                let _ = self.compcore.write_all(req.as_bytes());
            }
            Msg::Key { key, .. } => {
                if self.shown != Some(client) || self.cands.is_empty() {
                    return;
                }
                let n = self.cands.len();
                match key.as_str() {
                    "up" => self.selected = (self.selected + n - 1) % n,
                    "down" => self.selected = (self.selected + 1) % n,
                    "accept" => {
                        let Some(c) = &self.ctx else { return };
                        let m = Msg::Accept { v: 1, session: c.session, seq: c.seq, start: self.word.0, end: self.word.1, text: self.cands[self.selected].insert.clone() };
                        if let Some(s) = self.clients.get_mut(&client) {
                            reply(s, &m);
                        }
                        self.suppressed = Some((client, self.word.0));
                        // the wrapper stops taking keys on accept by itself
                        self.shown = None;
                        return self.popup.hide();
                    }
                    "dismiss" => {
                        self.suppressed = Some((client, self.word.0));
                        return self.hide();
                    }
                    _ => return,
                }
                // scrolls only as far as needed to keep the selection in view, as kuterm does
                self.top = self.top.min(self.selected).max((self.selected + 1).saturating_sub(ROWS));
                self.show();
            }
            _ => {}
        }
    }

    /// handles one event; false to quit
    pub fn on(&mut self, ev: Ev) -> bool {
        match ev {
            Ev::Client(id, s) => {
                self.clients.insert(id, s);
            }
            Ev::Msg(id, m) => self.on_msg(id, m),
            Ev::Gone(id) => {
                self.hide_if(id);
                self.clients.remove(&id);
                if self.clients.is_empty() {
                    let tx = self.tx.clone();
                    thread::spawn(move || {
                        thread::sleep(std::time::Duration::from_secs(30));
                        let _ = tx.send_blocking(Ev::Idle);
                    });
                }
            }
            // no wrapper left: the next one starts a host again
            Ev::Idle if self.clients.is_empty() => return false,
            Ev::Idle => {}
            Ev::Cands(rec) => self.on_cands(&rec),
            Ev::Window(app) => {
                // focus change, move or resize: the popup would sit in the wrong place
                if app != self.app {
                    log(&format!("window {app:?}"));
                    self.hide();
                }
                self.app = app;
            }
            Ev::Quit => return false,
        }
        true
    }
}

/// socket server: one thread per compui-pty, its messages go to `tx`
pub fn serve(tx: async_channel::Sender<Ev>) {
    let path = format!("/tmp/compui-{}.sock", unsafe { libc::getuid() });
    let _ = fs::remove_file(&path);
    let srv = UnixListener::bind(&path).expect("host socket");
    log(&format!("listening on {path}"));
    thread::spawn(move || {
        for (id, conn) in srv.incoming().map_while(Result::ok).enumerate() {
            let id = id as u64;
            let tx = tx.clone();
            let Ok(w) = conn.try_clone() else { continue };
            let _ = tx.send_blocking(Ev::Client(id, w));
            thread::spawn(move || {
                for line in BufReader::new(conn).lines().map_while(Result::ok) {
                    if let Ok(m) = serde_json::from_str::<Msg>(&line) {
                        let _ = tx.send_blocking(Ev::Msg(id, m));
                    }
                }
                let _ = tx.send_blocking(Ev::Gone(id));
            });
        }
    });
}

/// `compcore serve` on stdin/stdout, for the swift host
fn serve_stdio() {
    let _ = compcore::serve::run(BufReader::new(std::io::stdin()), std::io::stdout().lock(), &specs(), true);
}

#[cfg(target_os = "linux")]
fn host() {
    // one host per user: one started by a racing wrapper leaves
    let dir = runtime_dir();
    let _ = fs::create_dir_all(&dir);
    let lock = fs::File::create(dir.join("host.lock")).expect("host lock");
    if lock.try_lock().is_err() {
        return;
    }
    common::run(vec![Box::new(ghostty::Ghostty::default()), Box::new(konsole::Konsole::default())]);
}

#[cfg(not(target_os = "linux"))]
fn host() {
    eprintln!("the rust host runs on linux only; on macos build the swift host");
    std::process::exit(1);
}

fn main() {
    match env::args().nth(1).as_deref() {
        Some("host") => host(),
        Some("serve") => serve_stdio(),
        _ => pty::run(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reply_parses() {
        let (id, s, e, c) = parse_reply("7\u{1f}4\u{1f}6\u{1e}checkout\u{1f}checkout\u{1f}switch branches\u{1f}subcommand\u{1e}cherry\u{1f}cherry").unwrap();
        assert_eq!((id, s, e, c.len()), (7, 4, 6, 2));
        assert_eq!((c[0].desc.as_str(), c[0].kind.as_str(), c[1].kind.as_str()), ("switch branches", "subcommand", ""));
        assert!(parse_reply("7\u{1f}4").is_none());
    }

    #[test]
    fn config_parses() {
        assert_eq!(Config::parse(""), Config { enabled: true, debug: false });
        assert_eq!(Config::parse("# x\nenabled = false # off\ndebug=true\n"), Config { enabled: false, debug: true });
    }

    // reads /proc
    #[cfg(target_os = "linux")]
    #[test]
    fn ancestors() {
        assert!(is_ancestor(std::process::id(), std::process::id()));
        assert!(is_ancestor(ppid(std::process::id()).unwrap(), std::process::id()));
        assert!(!is_ancestor(u32::MAX, std::process::id()));
    }
}
