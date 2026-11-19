//! runs the user's shell in a pty, passes every byte through, reports the line and cursor cell to
//! compui-host and takes the popup keys while the host shows it
use compui_proto::{Cell, Grid, Msg};
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde_json::Value;
use std::io::{BufRead, BufReader, Read, Write};
use std::num::NonZeroU16 as NZ;
use std::os::unix::net::{UnixListener, UnixStream};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, process, thread};

struct State {
    parser: vt100::Parser,
    host: Option<UnixStream>,
    zsh: Option<UnixStream>,
    /// the shell's newest line, sent to the host once the screen has settled so `cell` is current
    pending: Option<Value>,
    last_output: Instant,
    shown: bool,
    seq: u64,
    path: String,
}

fn term_size() -> PtySize {
    let mut ws: libc::winsize = unsafe { std::mem::zeroed() };
    unsafe { libc::ioctl(0, libc::TIOCGWINSZ, &mut ws) };
    PtySize { rows: ws.ws_row.max(1), cols: ws.ws_col.max(1), pixel_width: ws.ws_xpixel, pixel_height: ws.ws_ypixel }
}

fn raw_mode() -> libc::termios {
    let mut t: libc::termios = unsafe { std::mem::zeroed() };
    unsafe {
        libc::tcgetattr(0, &mut t);
        let saved = t;
        libc::cfmakeraw(&mut t);
        libc::tcsetattr(0, libc::TCSANOW, &t);
        saved
    }
}

/// asks the terminal where its cursor is (1-based row, col), so the parser starts in sync with a screen that
/// already has output; None if it does not answer. shortcut: typed-ahead bytes read here are dropped
fn cursor_start() -> Option<(u16, u16)> {
    std::io::stdout().write_all(b"\x1b[6n").and_then(|_| std::io::stdout().flush()).ok()?;
    let (mut reply, mut b) = (Vec::new(), [0u8; 32]);
    let end = Instant::now() + Duration::from_millis(200);
    while !reply.ends_with(b"R") {
        let left = end.checked_duration_since(Instant::now())?.as_millis() as i32;
        let mut p = libc::pollfd { fd: 0, events: libc::POLLIN, revents: 0 };
        if unsafe { libc::poll(&mut p, 1, left) } <= 0 {
            return None;
        }
        let n = std::io::stdin().read(&mut b).ok().filter(|n| *n > 0)?;
        reply.extend_from_slice(&b[..n]);
    }
    let text = String::from_utf8_lossy(&reply);
    let (r, c) = text.rsplit_once("\x1b[")?.1.trim_end_matches('R').split_once(';')?;
    Some((r.parse().ok()?, c.parse().ok()?))
}

/// best effort: the popup is optional, the terminal must work without the host
fn send(st: &mut State, m: &Msg) {
    if let Some(s) = &mut st.host {
        let line = serde_json::to_string(m).unwrap() + "\n";
        if s.write_all(line.as_bytes()).is_err() {
            st.host = None;
        }
    }
}

/// the popup key a whole stdin read stands for, if any
fn popup_key(read: &[u8]) -> Option<&'static str> {
    match read {
        b"\x1b[A" | b"\x1bOA" => Some("up"),
        b"\x1b[B" | b"\x1bOB" => Some("down"),
        b"\t" => Some("accept"),
        b"\x1b" => Some("dismiss"),
        _ => None,
    }
}

fn main() {
    let size = term_size();
    let pair = native_pty_system().openpty(size).expect("openpty");
    let exe_dir = env::current_exe().unwrap().parent().unwrap().to_path_buf();
    let session = process::id();
    let zsh_sock = format!("/tmp/compui-{session}.zsh.sock");
    let _ = fs::remove_file(&zsh_sock);
    let listener = UnixListener::bind(&zsh_sock).expect("adapter socket");

    let shell = env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".into());
    let mut cmd = CommandBuilder::new(shell);
    cmd.arg("-l");
    cmd.env("COMPUI_SOCKET", &zsh_sock);
    let shell_dir = exe_dir.join("shell/zsh");
    cmd.env("COMPUI_SHELL_DIR", &shell_dir);
    cmd.env("ZDOTDIR", &shell_dir);
    if let Ok(z) = env::var("ZDOTDIR") {
        cmd.env("COMPUI_ZDOTDIR", z);
    }
    let mut child = pair.slave.spawn_command(cmd).expect("spawn shell");
    drop(pair.slave);

    let host = UnixStream::connect(format!("/tmp/compui-{}.sock", unsafe { libc::getuid() })).ok();
    let st = Arc::new(Mutex::new(State {
        parser: vt100::Parser::new(NZ::new(size.rows).unwrap(), NZ::new(size.cols).unwrap(), 0),
        host: host.as_ref().and_then(|h| h.try_clone().ok()),
        zsh: None,
        pending: None,
        last_output: Instant::now(),
        shown: false,
        seq: 0,
        path: env::var("PATH").unwrap_or_default(),
    }));

    // host -> wrapper: shown, hidden, accept (forwarded to zsh in kuterm's protocol v1)
    if let Some(h) = host {
        let st = st.clone();
        thread::spawn(move || {
            for line in BufReader::new(h).lines().map_while(Result::ok) {
                let mut st = st.lock().unwrap();
                match serde_json::from_str::<Msg>(&line) {
                    Ok(Msg::Shown { .. }) => st.shown = true,
                    Ok(Msg::Hidden { .. }) => st.shown = false,
                    Ok(Msg::Accept { seq, start, end, text, .. }) => {
                        st.shown = false;
                        if let Some(z) = &mut st.zsh {
                            let _ = writeln!(z, "1 accept {seq} {start} {end} {text}");
                        }
                    }
                    _ => {}
                }
            }
        });
    }

    // shell adapter -> wrapper
    {
        let st = st.clone();
        thread::spawn(move || {
            let Ok((conn, _)) = listener.accept() else { return };
            st.lock().unwrap().zsh = conn.try_clone().ok();
            for line in BufReader::new(conn).lines().map_while(Result::ok) {
                let Ok(v) = serde_json::from_str::<Value>(&line) else { continue };
                let mut st = st.lock().unwrap();
                match v["type"].as_str() {
                    Some("env") => st.path = v["path"].as_str().unwrap_or_default().into(),
                    Some("line") => {
                        st.seq = v["seq"].as_u64().unwrap_or(0);
                        st.pending = Some(v);
                    }
                    Some("end") => {
                        st.pending = None;
                        st.shown = false;
                        send(&mut st, &Msg::End { v: 1, session });
                    }
                    _ => {}
                }
            }
        });
    }

    // settled screen -> line message with the cursor cell
    {
        let st = st.clone();
        thread::spawn(move || loop {
            thread::sleep(Duration::from_millis(10));
            let mut st = st.lock().unwrap();
            if st.pending.is_none() || st.last_output.elapsed() < Duration::from_millis(15) {
                continue;
            }
            let v = st.pending.take().unwrap();
            let screen = st.parser.screen();
            if screen.alternate_screen() {
                continue;
            }
            let (row, col) = screen.cursor_position();
            let s = term_size();
            let m = Msg::Line {
                v: 1,
                session,
                seq: v["seq"].as_u64().unwrap_or(0),
                buffer: v["buffer"].as_str().unwrap_or_default().into(),
                cursor: v["cursor"].as_u64().unwrap_or(0) as usize,
                cwd: v["cwd"].as_str().unwrap_or_default().into(),
                path: st.path.clone(),
                history: v["history"].as_bool().unwrap_or(false),
                cell: Cell { row, col },
                grid: Grid { rows: s.rows, cols: s.cols, xpx: s.pixel_width, ypx: s.pixel_height },
            };
            send(&mut st, &m);
        });
    }

    let saved = raw_mode();
    if let Some((r, c)) = cursor_start() {
        st.lock().unwrap().parser.process(format!("\x1b[{r};{c}H").as_bytes());
    }
    let master = Arc::new(Mutex::new(pair.master));
    let mut reader = master.lock().unwrap().try_clone_reader().unwrap();
    let mut writer = master.lock().unwrap().take_writer().unwrap();

    // stdin -> shell; while the popup shows, a whole read that is exactly a popup key goes to the host
    {
        let st = st.clone();
        thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = std::io::stdin().read(&mut buf) {
                if n == 0 {
                    break;
                }
                let key = popup_key(&buf[..n]);
                let mut st = st.lock().unwrap();
                if let (true, Some(key)) = (st.shown && !st.parser.screen().alternate_screen(), key) {
                    let seq = st.seq;
                    send(&mut st, &Msg::Key { v: 1, session, seq, key: key.into() });
                    continue;
                }
                drop(st);
                if writer.write_all(&buf[..n]).is_err() {
                    break;
                }
            }
        });
    }

    // shortcut: polls the size, a SIGWINCH handler would react faster
    {
        let m2 = master.clone();
        let st = st.clone();
        thread::spawn(move || {
            let mut last = term_size();
            loop {
                thread::sleep(Duration::from_millis(200));
                let s = term_size();
                if (s.rows, s.cols) != (last.rows, last.cols) {
                    m2.lock().unwrap().resize(s).ok();
                    st.lock().unwrap().parser.screen_mut().set_size(NZ::new(s.rows).unwrap(), NZ::new(s.cols).unwrap());
                    last = s;
                }
            }
        });
    }

    let mut buf = [0u8; 8192];
    let mut out = std::io::stdout();
    while let Ok(n) = reader.read(&mut buf) {
        if n == 0 {
            break;
        }
        out.write_all(&buf[..n]).and_then(|_| out.flush()).ok();
        let mut st = st.lock().unwrap();
        st.parser.process(&buf[..n]);
        st.last_output = Instant::now();
        if st.shown && st.parser.screen().alternate_screen() {
            st.shown = false;
            send(&mut st, &Msg::End { v: 1, session });
        }
    }

    let mut st = st.lock().unwrap();
    send(&mut st, &Msg::End { v: 1, session });
    let _ = fs::remove_file(&zsh_sock);
    unsafe { libc::tcsetattr(0, libc::TCSANOW, &saved) };
    process::exit(child.wait().map(|s| s.exit_code() as i32).unwrap_or(1));
}

#[cfg(test)]
mod tests {
    use super::popup_key;

    #[test]
    fn popup_keys() {
        assert_eq!(popup_key(b"\x1b[A"), Some("up"));
        assert_eq!(popup_key(b"\x1bOB"), Some("down"));
        assert_eq!(popup_key(b"\t"), Some("accept"));
        assert_eq!(popup_key(b"\x1b"), Some("dismiss"));
        assert_eq!(popup_key(b"\x1b[A\x1b[A"), None);
        assert_eq!(popup_key(b"a\t"), None);
    }
}
