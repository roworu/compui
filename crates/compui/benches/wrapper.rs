//! wrapper benchmarks against a fake host: startup, keystroke echo, line and popup key latency,
//! output throughput, idle wakeups, memory and host reconnect, with plain zsh as the baseline.
//! `cargo bench -p compui`; each run is saved to target/bench/last.json and compared with the one
//! before. `-- --save NAME` also keeps it as NAME.json, `-- --vs NAME` compares with that instead
use portable_pty::{Child, CommandBuilder, PtySize, native_pty_system};
use serde_json::{Map, Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{env, fs, thread};

const WAIT: Duration = Duration::from_secs(10);
const PROMPT: &[u8] = b"bench$ ";
const RUNS: usize = 5;
const SAMPLES: usize = 100;
const BYTES: usize = 20_000_000;

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// a program in a pty of its own, its output as timestamped chunks
struct Term {
    child: Box<dyn Child + Send + Sync>,
    writer: Writer,
    out: Receiver<(Instant, Vec<u8>)>,
    tail: Vec<u8>,
}

impl Term {
    fn spawn(cmd: CommandBuilder) -> Term {
        let size = PtySize { rows: 40, cols: 120, pixel_width: 0, pixel_height: 0 };
        let pair = native_pty_system().openpty(size).unwrap();
        let child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);
        let writer: Writer = Arc::new(Mutex::new(pair.master.take_writer().unwrap()));
        let mut reader = pair.master.try_clone_reader().unwrap();
        let (tx, out) = channel();
        let w = writer.clone();
        thread::spawn(move || {
            let _master = pair.master;
            let mut buf = [0u8; 65536];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                let now = Instant::now();
                // the wrapper asks where the cursor is when it starts
                if find(&buf[..n], b"\x1b[6n").is_some() {
                    let _ = w.lock().unwrap().write_all(b"\x1b[1;1R");
                }
                if tx.send((now, buf[..n].to_vec())).is_err() {
                    break;
                }
            }
        });
        Term { child, writer, out, tail: Vec::new() }
    }

    /// writes `b`, returns when it started
    fn send(&self, b: &[u8]) -> Instant {
        let mut w = self.writer.lock().unwrap();
        let t = Instant::now();
        w.write_all(b).and_then(|_| w.flush()).unwrap();
        t
    }

    fn drain(&mut self) {
        while self.out.try_recv().is_ok() {}
        self.tail.clear();
    }

    /// when the next output arrives
    fn next(&mut self) -> Instant {
        self.out.recv_timeout(WAIT).expect("no output").0
    }

    /// when `pat` shows up in the output
    fn wait_for(&mut self, pat: &[u8]) -> Instant {
        let end = Instant::now() + WAIT;
        loop {
            let Ok((t, b)) = self.out.recv_timeout(end.saturating_duration_since(Instant::now())) else {
                panic!("no {:?} in the output", String::from_utf8_lossy(pat));
            };
            self.tail.extend_from_slice(&b);
            if let Some(i) = find(&self.tail, pat) {
                self.tail.drain(..i + pat.len());
                return t;
            }
            let keep = self.tail.len().saturating_sub(pat.len());
            self.tail.drain(..keep);
        }
    }

    fn pid(&self) -> u32 {
        self.child.process_id().unwrap()
    }

    fn close(mut self) {
        let _ = self.send(b"\x15exit\r");
        let end = Instant::now() + Duration::from_secs(3);
        while Instant::now() < end {
            if let Ok(Some(_)) = self.child.try_wait() {
                return;
            }
            thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
    }
}

/// stands in for the host: records what the wrappers send, can talk back and hang up
struct Host {
    msgs: Receiver<(Instant, Value)>,
    conns: Receiver<Instant>,
    conn: Arc<Mutex<Option<UnixStream>>>,
}

impl Host {
    fn start(path: &Path) -> Host {
        let srv = UnixListener::bind(path).unwrap();
        let (mtx, msgs) = channel();
        let (ctx, conns) = channel();
        let conn = Arc::new(Mutex::new(None));
        let c = conn.clone();
        thread::spawn(move || {
            for s in srv.incoming().map_while(Result::ok) {
                let _ = ctx.send(Instant::now());
                *c.lock().unwrap() = s.try_clone().ok();
                let mtx = mtx.clone();
                thread::spawn(move || {
                    for line in BufReader::new(s).lines().map_while(Result::ok) {
                        let t = Instant::now();
                        if let Ok(v) = serde_json::from_str(&line) {
                            let _ = mtx.send((t, v));
                        }
                    }
                });
            }
        });
        Host { msgs, conns, conn }
    }

    /// when the next message of `kind` arrived
    fn recv(&self, kind: &str) -> Instant {
        let end = Instant::now() + WAIT;
        loop {
            let Ok((t, v)) = self.msgs.recv_timeout(end.saturating_duration_since(Instant::now())) else {
                panic!("no {kind} message from the wrapper");
            };
            if v["type"] == kind {
                return t;
            }
        }
    }

    fn drain(&self) {
        while self.msgs.try_recv().is_ok() {}
        while self.conns.try_recv().is_ok() {}
    }

    fn say(&self, v: Value) {
        if let Some(s) = &mut *self.conn.lock().unwrap() {
            let _ = writeln!(s, "{v}");
        }
    }

    /// closes the newest connection, like a host that crashed
    fn hang_up(&self) {
        if let Some(s) = self.conn.lock().unwrap().take() {
            let _ = s.shutdown(std::net::Shutdown::Both);
        }
    }
}

fn find(hay: &[u8], pat: &[u8]) -> Option<usize> {
    hay.windows(pat.len()).position(|w| w == pat)
}

fn us(d: Duration) -> f64 {
    d.as_secs_f64() * 1e6
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1e3
}

/// the `p`th percentile, 0..=100
fn pct(v: &[f64], p: usize) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    v[(v.len() - 1) * p / 100]
}

fn which(name: &str) -> String {
    env::split_paths(&env::var_os("PATH").unwrap_or_default())
        .map(|d| d.join(name))
        .find(|p| p.is_file())
        .unwrap_or_else(|| panic!("no {name} in PATH"))
        .to_string_lossy()
        .into()
}

/// `prog` with a bare zsh config and none of the compui session this may run inside
fn command(prog: &str, args: &[&str], dir: &Path, zsh: &str) -> CommandBuilder {
    let mut cmd = CommandBuilder::new(prog);
    cmd.args(args);
    for k in ["COMPUI_SOCKET", "COMPUI_SHELL_DIR", "COMPUI_ZDOTDIR", "COMPUI_DEBUG"] {
        cmd.env_remove(k);
    }
    cmd.env("ZDOTDIR", dir.join("zdot"));
    cmd.env("XDG_RUNTIME_DIR", dir);
    cmd.env("COMPUI_HOST_SOCKET", dir.join("host.sock"));
    cmd.env("SHELL", zsh);
    cmd.env("TERM", "xterm-256color");
    cmd.cwd(dir);
    cmd
}

/// echo latency per keystroke, and with a host also the keystroke to `line` latency
fn typing(t: &mut Term, host: Option<&Host>) -> (Vec<f64>, Vec<f64>) {
    let (mut echo, mut line) = (Vec::new(), Vec::new());
    for i in 0..SAMPLES {
        // ctrl-u now and then keeps the line short
        if i % 40 == 0 {
            t.send(b"\x15");
            thread::sleep(Duration::from_millis(50));
            t.drain();
            host.map(Host::drain);
        }
        let t0 = t.send(b"a");
        echo.push(us(t.next() - t0));
        if let Some(h) = host {
            line.push(us(h.recv("line") - t0));
        }
        thread::sleep(Duration::from_millis(30));
        t.drain();
        host.map(Host::drain);
    }
    (echo, line)
}

/// MB/s of `cat`ing BYTES of text until its end marker
fn throughput(t: &mut Term, dir: &Path) -> f64 {
    t.drain();
    let t0 = t.send(format!("\x15cat {}; echo $((6*7))done\r", dir.join("out.txt").display()).as_bytes());
    let t1 = t.wait_for(b"42done");
    t.wait_for(PROMPT);
    BYTES as f64 / (t1 - t0).as_secs_f64() / 1e6
}

/// context switches and nanoseconds on cpu, summed over the process's threads
fn sched(pid: u32) -> Option<(u64, u64)> {
    let (mut sw, mut ns) = (0, 0);
    for task in fs::read_dir(format!("/proc/{pid}/task")).ok()?.flatten() {
        let status = fs::read_to_string(task.path().join("status")).unwrap_or_default();
        for l in status.lines() {
            if let Some(v) = l.strip_prefix("voluntary_ctxt_switches:").or(l.strip_prefix("nonvoluntary_ctxt_switches:")) {
                sw += v.trim().parse::<u64>().unwrap_or(0);
            }
        }
        let st = fs::read_to_string(task.path().join("schedstat")).unwrap_or_default();
        ns += st.split_whitespace().next().and_then(|v| v.parse::<u64>().ok()).unwrap_or(0);
    }
    Some((sw, ns))
}

/// rss and private memory in kB
fn memory(pid: u32) -> Option<(f64, f64)> {
    let text = fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    let kb = |k: &str| -> f64 {
        text.lines().find_map(|l| l.strip_prefix(k)).and_then(|v| v.split_whitespace().next()?.parse().ok()).unwrap_or(0.0)
    };
    Some((kb("Rss:"), kb("Private_Clean:") + kb("Private_Dirty:")))
}

struct Results(Vec<(&'static str, Option<f64>)>);

impl Results {
    fn add(&mut self, name: &'static str, v: impl Into<Option<f64>>) {
        let v = v.into();
        eprintln!("  {name}: {}", v.map_or("-".into(), |v| format!("{v:.1}")));
        self.0.push((name, v));
    }
}

fn main() {
    let args: Vec<String> = env::args().collect();
    let flag = |f: &str| args.iter().position(|a| a == f).and_then(|i| args.get(i + 1)).cloned();
    let exe = env!("CARGO_BIN_EXE_compui");
    let zsh = which("zsh");
    let dir = env::temp_dir().join(format!("compui-bench-{}", std::process::id()));
    fs::create_dir_all(dir.join("zdot")).unwrap();
    fs::write(dir.join("out.txt"), format!("{}\n", "x".repeat(99)).repeat(BYTES / 100)).unwrap();
    fs::write(dir.join("zdot/.zshrc"), "PROMPT='bench$ '\nunsetopt prompt_sp\n").unwrap();
    let host = Host::start(&dir.join("host.sock"));
    let cmd = |prog: &str, args: &[&str]| command(prog, args, &dir, &zsh);
    let mut r = Results(Vec::new());

    eprintln!("startup ({RUNS} runs)");
    let mut v = Vec::new();
    for _ in 0..RUNS {
        let t0 = Instant::now();
        let mut t = Term::spawn(cmd(&zsh, &["-l"]));
        v.push(ms(t.wait_for(PROMPT) - t0));
        t.close();
    }
    r.add("startup_zsh_ms", pct(&v, 50));
    let (mut prompt, mut ready) = (Vec::new(), Vec::new());
    for _ in 0..RUNS {
        host.drain();
        let t0 = Instant::now();
        let mut t = Term::spawn(cmd(exe, &[]));
        prompt.push(ms(t.wait_for(PROMPT) - t0));
        ready.push(ms(host.recv("line") - t0));
        t.close();
    }
    r.add("startup_prompt_ms", pct(&prompt, 50));
    r.add("startup_ready_ms", pct(&ready, 50));

    eprintln!("plain zsh");
    let mut t = Term::spawn(cmd(&zsh, &["-l"]));
    t.wait_for(PROMPT);
    let (echo, _) = typing(&mut t, None);
    r.add("echo_zsh_p50_us", pct(&echo, 50));
    r.add("echo_zsh_p99_us", pct(&echo, 99));
    r.add("throughput_zsh_mbs", throughput(&mut t, &dir));
    t.close();

    eprintln!("wrapper");
    host.drain();
    let mut t = Term::spawn(cmd(exe, &[]));
    t.wait_for(PROMPT);
    host.recv("line");
    thread::sleep(Duration::from_millis(500));
    let pid = t.pid();
    let mem = memory(pid);
    r.add("rss_kb", mem.map(|m| m.0));
    r.add("private_kb", mem.map(|m| m.1));
    let a = sched(pid);
    thread::sleep(Duration::from_secs(5));
    let idle = a.zip(sched(pid)).map(|(a, b)| ((b.0 - a.0) as f64 / 5.0, (b.1 - a.1) as f64 / 5e3));
    r.add("idle_wakeups_per_s", idle.map(|i| i.0));
    r.add("idle_cpu_us_per_s", idle.map(|i| i.1));

    let (echo, line) = typing(&mut t, Some(&host));
    r.add("echo_p50_us", pct(&echo, 50));
    r.add("echo_p99_us", pct(&echo, 99));
    r.add("line_p50_us", pct(&line, 50));
    r.add("line_p99_us", pct(&line, 99));

    host.say(json!({"type": "shown", "v": 1, "session": 0}));
    thread::sleep(Duration::from_millis(50));
    host.drain();
    let mut key = Vec::new();
    for _ in 0..SAMPLES {
        let t0 = t.send(b"\x1b[B");
        key.push(us(host.recv("key") - t0));
    }
    host.say(json!({"type": "hidden", "v": 1, "session": 0}));
    thread::sleep(Duration::from_millis(50));
    r.add("key_p50_us", pct(&key, 50));
    r.add("key_p99_us", pct(&key, 99));

    r.add("throughput_mbs", throughput(&mut t, &dir));

    host.drain();
    let t0 = Instant::now();
    host.hang_up();
    r.add("reconnect_ms", host.conns.recv_timeout(Duration::from_secs(5)).ok().map(|t1| ms(t1 - t0)));
    t.close();
    let _ = fs::remove_dir_all(&dir);

    report(&r, flag("--vs").as_deref().unwrap_or("last"), flag("--save").as_deref());
}

/// prints the results next to an earlier run, then saves them
fn report(r: &Results, vs: &str, save: Option<&str>) {
    let out = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/bench");
    fs::create_dir_all(&out).unwrap();
    let prev: Map<String, Value> = fs::read_to_string(out.join(format!("{vs}.json")))
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    let fmt = |v: Option<f64>| v.map_or("-".into(), |v| format!("{v:.1}"));
    println!("\n{:<22} {:>12} {:>12} {:>9}", "metric", vs, "now", "change");
    for (name, v) in &r.0 {
        let p = prev.get(*name).and_then(Value::as_f64);
        let change = p.zip(*v).filter(|(p, _)| *p != 0.0).map_or(String::new(), |(p, v)| format!("{:+.0}%", (v - p) / p * 100.0));
        println!("{name:<22} {:>12} {:>12} {change:>9}", fmt(p), fmt(*v));
    }
    let json = Value::Object(r.0.iter().map(|(k, v)| (k.to_string(), json!(v))).collect()).to_string();
    fs::write(out.join("last.json"), &json).unwrap();
    if let Some(name) = save {
        fs::write(out.join(format!("{name}.json")), &json).unwrap();
    }
}
