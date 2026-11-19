//! linux side of the host (`compui host`, started by the first wrapper): a layer-shell
//! popup (gtk4, `popup.rs`) and a kwin script that reports the active window over d-bus. kde plasma
//! (wayland) only for now
#[path = "popup.rs"]
mod popup;

use crate::{App, Ev, Grid, Handler, Host, Rect, serve};
use gtk4::{self as gtk, glib};
use popup::Gtk;
use std::{env, fs};
use zbus::blocking::Connection;

const SVC: &str = "io.github.roworu.compui";

/// receives the kwin script's window reports
struct Bridge(async_channel::Sender<Ev>);

#[zbus::interface(name = "io.github.roworu.compui")]
impl Bridge {
    fn window(&self, json: String) {
        let v: serde_json::Value = serde_json::from_str(&json).unwrap_or_default();
        let pid = v["pid"].as_u64().unwrap_or(0) as u32;
        let f = |k: &str| v[k].as_f64().unwrap_or(0.0);
        let app = if pid == 0 {
            App::default()
        } else {
            App {
                pid,
                exe: fs::read_to_string(format!("/proc/{pid}/comm")).unwrap_or_default().trim().into(),
                rect: Rect { x: f("x"), y: f("y"), w: f("w"), h: f("h") },
                output: v["output"].as_str().unwrap_or_default().into(),
                scale: f("scale").max(1.0),
            }
        };
        let _ = self.0.send_blocking(Ev::Window(app));
    }
}

/// cell and text area size in logical px from the pty's pixel size, which is the text area with the
/// leftover pixels; cells are whole pixels and the leftover is under one cell, so the floor is the cell
/// as long as it is smaller than cols / rows
pub fn cells(g: &Grid, scale: f64) -> Option<(f64, f64, f64, f64)> {
    if g.xpx == 0 || g.ypx == 0 || g.cols == 0 || g.rows == 0 {
        return None;
    }
    Some(((g.xpx / g.cols) as f64 / scale, (g.ypx / g.rows) as f64 / scale, g.xpx as f64 / scale, g.ypx as f64 / scale))
}

/// the word's cell when the text area starts `left` in from the client rect's left and ends `bottom` up
/// from its bottom. the top is found from the bottom since a header, menu, tool or tab bar of unknown
/// height sits above
pub fn cell_rect(app: &App, g: &Grid, scale: f64, left: f64, bottom: f64) -> Option<Rect> {
    let (cw, ch, _, gh) = cells(g, scale)?;
    let top = app.rect.y + app.rect.h - bottom - gh;
    Some(Rect { x: app.rect.x + left + g.col as f64 * cw, y: top + g.row as f64 * ch, w: cw, h: ch })
}

fn kwin_call<B: serde::Serialize + zbus::zvariant::DynamicType>(c: &Connection, path: &str, iface: &str, method: &str, body: &B) -> zbus::Result<zbus::Message> {
    c.call_method(Some("org.kde.KWin"), path, Some(iface), method, body)
}

/// owns the bridge's d-bus name and (re)loads the kwin script
fn kwin(tx: async_channel::Sender<Ev>) -> zbus::Result<Connection> {
    let conn = zbus::blocking::connection::Builder::session()?.name(SVC)?.serve_at("/", Bridge(tx))?.build()?;
    let script = env::temp_dir().join(format!("compui-{}-kwin.js", unsafe { libc::getuid() }));
    fs::write(&script, include_str!("kwin.js")).map_err(|e| zbus::Error::Failure(e.to_string()))?;
    // a host that was killed leaves its script loaded
    let _ = kwin_call(&conn, "/Scripting", "org.kde.kwin.Scripting", "unloadScript", &("compui"));
    let id: i32 = kwin_call(&conn, "/Scripting", "org.kde.kwin.Scripting", "loadScript", &(script.to_string_lossy().as_ref(), "compui"))?.body().deserialize()?;
    if id < 0 {
        return Err(zbus::Error::Failure("kwin refused the script".into()));
    }
    kwin_call(&conn, &format!("/Scripting/Script{id}"), "org.kde.kwin.Script", "run", &())?;
    Ok(conn)
}

/// runs the host until sigterm or sigint
pub fn run(handlers: Vec<Box<dyn Handler>>) {
    // blocked before any thread starts so only the sigwait thread below gets them
    let mut sigs: libc::sigset_t = unsafe { std::mem::zeroed() };
    unsafe {
        libc::sigemptyset(&mut sigs);
        libc::sigaddset(&mut sigs, libc::SIGINT);
        libc::sigaddset(&mut sigs, libc::SIGTERM);
        libc::pthread_sigmask(libc::SIG_BLOCK, &sigs, std::ptr::null_mut());
    }
    gtk::init().expect("gtk needs a wayland display");
    let (tx, rx) = async_channel::unbounded();
    // without these the popup can't show; said always, so the host log tells why
    if !gtk4_layer_shell::is_supported() {
        eprintln!("compositor without layer-shell, no popup");
    }
    let bridge = kwin(tx.clone()).inspect_err(|e| eprintln!("kwin script not loaded, no popup: {e}"));
    let mut host = Host::new(Gtk::new(), handlers, tx.clone());
    host.reload();
    serve(tx.clone());
    {
        let tx = tx.clone();
        std::thread::spawn(move || {
            let mut sig = 0;
            unsafe { libc::sigwait(&sigs, &mut sig) };
            let _ = tx.send_blocking(Ev::Quit);
        });
    }
    let main = glib::MainLoop::new(None, false);
    let m = main.clone();
    glib::spawn_future_local(async move {
        while let Ok(ev) = rx.recv().await {
            if !host.on(ev) {
                break;
            }
        }
        m.quit();
    });
    main.run();
    if let Ok(c) = &bridge {
        let _ = kwin_call(c, "/Scripting", "org.kde.kwin.Scripting", "unloadScript", &("compui"));
    }
}
