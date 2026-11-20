//! gnome: mutter has no layer-shell and no kwin scripting, so a gnome shell extension (`extension.js`,
//! embedded here and written to the user's extensions dir) reports the focused window and shows the
//! popup. the host renders the popup into an image with the layer-shell popup's drawing code
use super::popup::{self, SHADOW};
use crate::{App, Cand, Popup, Rect, log, runtime_dir};
use gtk4::{cairo, gdk, prelude::*};
use std::{env, fs, path::PathBuf};
use zbus::blocking::Connection;

const UUID: &str = "compui@roworu.github.io";
const FILES: [(&str, &str); 2] = [("metadata.json", include_str!("metadata.json")), ("extension.js", include_str!("extension.js"))];

/// true in a gnome session
pub fn is_gnome() -> bool {
    env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|d| d.eq_ignore_ascii_case("gnome")))
}

/// writes the extension where gnome shell looks for it when missing or changed, and enables it. on
/// wayland the shell only finds a new extension after the next login
pub fn install(bus: &Connection) {
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    let data = env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let dir = data.join("gnome-shell/extensions").join(UUID);
    if FILES.iter().any(|(f, t)| fs::read_to_string(dir.join(f)).ok().as_deref() != Some(*t)) {
        let _ = fs::create_dir_all(&dir);
        for (f, t) in FILES {
            let _ = fs::write(dir.join(f), t);
        }
        eprintln!("gnome shell extension written to {}", dir.display());
    }
    let enabled = bus
        .call_method(Some("org.gnome.Shell.Extensions"), "/org/gnome/Shell/Extensions", Some("org.gnome.Shell.Extensions"), "EnableExtension", &(UUID))
        .and_then(|m| m.body().deserialize::<bool>());
    if !matches!(enabled, Ok(true)) {
        eprintln!("gnome shell extension {UUID} not enabled ({enabled:?}), no popup; log out and in once so gnome shell finds it");
    }
}

pub struct Shell {
    bus: Option<Connection>,
    /// the color scheme, read when the popup comes up
    dark: Option<bool>,
}

impl Shell {
    pub fn new(bus: Option<Connection>) -> Self {
        Shell { bus, dark: None }
    }
}

/// the monitor holding `cell`'s top-left, else the first
fn monitor(cell: Rect) -> Option<Rect> {
    let ms = gdk::Display::default()?.monitors();
    let all: Vec<Rect> = (0..ms.n_items())
        .filter_map(|i| ms.item(i)?.downcast::<gdk::Monitor>().ok())
        .map(|m| m.geometry())
        .map(|g| Rect { x: g.x() as f64, y: g.y() as f64, w: g.width() as f64, h: g.height() as f64 })
        .collect();
    all.iter().find(|m| cell.x >= m.x && cell.x < m.x + m.w && cell.y >= m.y && cell.y < m.y + m.h).or(all.first()).copied()
}

impl Popup for Shell {
    fn show(&mut self, app: &App, cell: Rect, cands: &[Cand], selected: usize, top: usize) {
        let Some(bus) = &self.bus else { return };
        let Some(mon) = monitor(cell) else { return };
        let dark = *self.dark.get_or_insert_with(|| popup::dark(Some(bus)));
        let ctx = pangocairo::FontMap::default().create_context();
        let (view, x, y) = popup::arrange(&ctx, mon, dark, cell, cands, selected, top);
        // drawn at the monitor's scale so it stays sharp; the shell sizes it in logical px
        let s = app.scale.max(1.0);
        let (iw, ih) = (view.w + SHADOW.0 + SHADOW.2, view.h + SHADOW.1 + SHADOW.3);
        let Ok(mut img) = cairo::ImageSurface::create(cairo::Format::ARgb32, (iw * s).ceil() as i32, (ih * s).ceil() as i32) else { return };
        if let Ok(cr) = cairo::Context::new(&img) {
            cr.scale(s, s);
            popup::draw(&view, &cr);
        }
        img.flush();
        let (pw, ph, stride) = (img.width(), img.height(), img.stride());
        // renamed into place, so the shell never reads a half written image
        let dir = runtime_dir();
        let (tmp, file) = (dir.join("popup.tmp"), dir.join("popup.argb"));
        let Ok(data) = img.data() else { return };
        if fs::write(&tmp, &*data).and_then(|_| fs::rename(&tmp, &file)).is_err() {
            return;
        }
        log(&format!("popup at {x},{y} size {}x{} scale {s}", view.w, view.h));
        let path = file.to_string_lossy();
        let args = (path.as_ref(), pw, ph, stride, x - SHADOW.0, y - SHADOW.1, iw, ih);
        if let Err(e) = bus.call_method(Some("org.gnome.Shell"), "/io/github/roworu/compui", Some("io.github.roworu.compui.Shell"), "Show", &args) {
            log(&format!("gnome shell extension: {e}"));
        }
    }

    fn hide(&mut self) {
        self.dark = None;
        if let Some(bus) = &self.bus {
            let _ = bus.call_method(Some("org.gnome.Shell"), "/io/github/roworu/compui", Some("io.github.roworu.compui.Shell"), "Hide", &());
        }
    }
}
