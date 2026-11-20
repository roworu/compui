//! alacritty on linux: the cell comes from the window's client rect and the pty's pixel size, which
//! alacritty sets to the grid alone in device pixels. no header or tab bar, so the grid sits at the
//! client's top-left plus the padding from alacritty's config
use crate::common::cells;
use crate::{App, Grid, Handler, Rect};
use std::{env, fs, path::PathBuf};

/// `[window]` padding in logical px and dynamic_padding
#[derive(Clone, Copy, Debug, PartialEq, Default)]
pub struct Padding {
    pub x: f64,
    pub y: f64,
    pub dynamic: bool,
}

/// reads window.padding (inline table, `[window.padding]` or dotted keys) and window.dynamic_padding
/// from alacritty's toml, default 0. shortcut: a line-based read that ignores `import`s
pub fn padding(config: &str) -> Padding {
    let mut p = Padding::default();
    let mut section = String::new();
    for line in config.lines() {
        let line = line.split('#').next().unwrap_or_default().trim();
        if let Some(s) = line.strip_prefix('[') {
            section = s.trim_end_matches(']').trim().into();
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let key = if section.is_empty() { k.trim().to_string() } else { format!("{section}.{}", k.trim()) };
        let v = v.trim();
        match key.as_str() {
            "window.padding" => {
                for kv in v.trim_matches(|c| c == '{' || c == '}').split(',') {
                    let Some((k, v)) = kv.split_once('=') else { continue };
                    let Ok(n) = v.trim().parse() else { continue };
                    match k.trim() {
                        "x" => p.x = n,
                        "y" => p.y = n,
                        _ => {}
                    }
                }
            }
            "window.padding.x" => p.x = v.parse().unwrap_or(p.x),
            "window.padding.y" => p.y = v.parse().unwrap_or(p.y),
            "window.dynamic_padding" => p.dynamic = v == "true",
            _ => {}
        }
    }
    p
}

/// the first config alacritty would load
fn config_text() -> String {
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    let xdg = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    [xdg.join("alacritty/alacritty.toml"), xdg.join("alacritty.toml"), home.join(".config/alacritty/alacritty.toml"), home.join(".alacritty.toml")]
        .iter()
        .find_map(|f| fs::read_to_string(f).ok())
        .unwrap_or_default()
}

pub struct Alacritty {
    pad: Padding,
}

impl Default for Alacritty {
    fn default() -> Self {
        Alacritty { pad: padding(&config_text()) }
    }
}

/// the cell rect from the client's top-left plus the padding, which alacritty scales and floors to
/// device px; dynamic padding adds half the leftover that is not a whole cell
pub fn locate(pad: Padding, app: &App, g: &Grid) -> Option<Rect> {
    let s = app.scale;
    let (cw, ch, _, _) = cells(g, 1.0)?;
    let inset = |p: f64, dim: f64, cell: f64| {
        let p = (p * s).floor();
        if pad.dynamic { p + (dim * s - 2.0 * p) % cell / 2.0 } else { p }
    };
    let (x, y) = (inset(pad.x, app.rect.w, cw), inset(pad.y, app.rect.h, ch));
    Some(Rect { x: app.rect.x + (x + g.col as f64 * cw) / s, y: app.rect.y + (y + g.row as f64 * ch) / s, w: cw / s, h: ch / s })
}

impl Handler for Alacritty {
    fn matches(&self, app: &App) -> bool {
        app.exe == "alacritty"
    }
    fn locate(&self, app: &App, g: &Grid) -> Option<Rect> {
        let r = locate(self.pad, app, g);
        crate::log(&format!("alacritty window {:?} scale {} {:?} grid {}x{} px {}x{} -> {r:?}", app.rect, app.scale, self.pad, g.cols, g.rows, g.xpx, g.ypx));
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_parses() {
        assert_eq!(padding(""), Padding::default());
        assert_eq!(padding("[window]\npadding = { x = 10, y = 4 } # c\ndynamic_padding = true\n"), Padding { x: 10.0, y: 4.0, dynamic: true });
        assert_eq!(padding("[window.padding]\nx = 3\ny = 5\n[font]\nx = 9\n"), Padding { x: 3.0, y: 5.0, dynamic: false });
        assert_eq!(padding("window.padding.x = 6\n"), Padding { x: 6.0, y: 0.0, dynamic: false });
    }

    #[test]
    fn cell_from_top() {
        // 80x24 cells of 10x20 px at scale 2 (5x10 points), padding 3 points, 7 px leftover each way
        let g = Grid { row: 3, col: 4, rows: 24, cols: 80, xpx: 800, ypx: 480 };
        let app = App { rect: Rect { x: 100.0, y: 50.0, w: (12.0 + 800.0 + 7.0) / 2.0, h: (12.0 + 480.0 + 7.0) / 2.0 }, scale: 2.0, ..App::default() };
        let pad = Padding { x: 3.0, y: 3.0, dynamic: false };
        assert_eq!(locate(pad, &app, &g), Some(Rect { x: 100.0 + 3.0 + 20.0, y: 50.0 + 3.0 + 30.0, w: 5.0, h: 10.0 }));
        // dynamic: half the 7 px leftover more
        let dynamic = Padding { dynamic: true, ..pad };
        assert_eq!(locate(dynamic, &app, &g), Some(Rect { x: 100.0 + 3.0 + 1.75 + 20.0, y: 50.0 + 3.0 + 1.75 + 30.0, w: 5.0, h: 10.0 }));
    }
}
