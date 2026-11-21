//! ghostty on linux: no text element to ask, so the cell comes from the window's client rect, the pty's
//! pixel size and the padding from ghostty's config
use crate::common::{cell_rect, cells};
use crate::{App, Grid, Handler, Rect};
use std::{env, fs, path::PathBuf};

/// window-padding in points: left, right, bottom
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Padding {
    pub left: f64,
    pub right: f64,
    pub bottom: f64,
}

/// reads window-padding-x / -y from ghostty's config text, default 2.
/// shortcut: ignores config-file includes and window-padding-balance
pub fn padding(config: &str) -> Padding {
    let mut p = Padding { left: 2.0, right: 2.0, bottom: 2.0 };
    for line in config.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        // "a" is both sides, "a,b" is left,right or top,bottom
        let vals: Vec<f64> = v.split(',').filter_map(|s| s.trim().parse().ok()).collect();
        let (Some(a), b) = (vals.first().copied(), vals.get(1).copied()) else { continue };
        match k.trim() {
            "window-padding-x" => (p.left, p.right) = (a, b.unwrap_or(a)),
            "window-padding-y" => p.bottom = b.unwrap_or(a),
            _ => {}
        }
    }
    p
}

fn config_text() -> String {
    let dir = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from(env::var_os("HOME").unwrap_or_default()).join(".config"));
    ["config", "config.ghostty"].iter().filter_map(|f| fs::read_to_string(dir.join("ghostty").join(f)).ok()).collect::<Vec<_>>().join("\n")
}

pub struct Ghostty {
    pad: Padding,
}

impl Default for Ghostty {
    fn default() -> Self {
        Ghostty { pad: padding(&config_text()) }
    }
}

/// the cell rect from the client's left plus the padding and its bottom minus the padding
pub fn locate(pad: Padding, app: &App, g: &Grid) -> Option<Rect> {
    // ghostty's pixel sizes are device pixels at gtk's integer scale, so 2 on a 1.25 output
    let scale = app.scale.ceil();
    let (cw, _, gw, _) = cells(g, scale)?;
    // split check: the grid must fill the window's width, else this is one pane of several
    if (app.rect.w - pad.left - pad.right - gw).abs() > cw {
        return None;
    }
    cell_rect(app, g, scale, pad.left, pad.bottom)
}

impl Handler for Ghostty {
    fn matches(&self, app: &App) -> bool {
        app.exe == "ghostty"
    }
    fn locate(&self, app: &App, g: &Grid) -> Option<Rect> {
        let r = locate(self.pad, app, g);
        crate::log(&format!("ghostty window {:?} scale {} grid {}x{} px {}x{} -> {r:?}", app.rect, app.scale, g.cols, g.rows, g.xpx, g.ypx));
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn padding_parses() {
        assert_eq!(padding(""), Padding { left: 2.0, right: 2.0, bottom: 2.0 });
        assert_eq!(padding("window-padding-x = 10\nwindow-padding-y = 4,8\n# x"), Padding { left: 10.0, right: 10.0, bottom: 8.0 });
        assert_eq!(padding("window-padding-x=3,5"), Padding { left: 3.0, right: 5.0, bottom: 2.0 });
    }

    #[test]
    fn cell_from_bottom() {
        // 80x24 cells of 10x20 px at scale 2 (5x10 points), 3 px leftover each way, window with a 40 pt header
        let pad = Padding { left: 2.0, right: 2.0, bottom: 2.0 };
        let gw = (800.0 + 3.0) / 2.0;
        let gh = (480.0 + 3.0) / 2.0;
        let app = App { rect: Rect { x: 100.0, y: 50.0, w: gw + 4.0, h: 40.0 + 2.0 + gh + 2.0 }, scale: 2.0, ..App::default() };
        let g = Grid { row: 3, col: 4, rows: 24, cols: 80, xpx: 803, ypx: 483 };
        let r = locate(pad, &app, &g).unwrap();
        assert_eq!(r, Rect { x: 100.0 + 2.0 + 20.0, y: 50.0 + 40.0 + 2.0 + 30.0, w: 5.0, h: 10.0 });
        // half the width: a split
        let half = App { rect: Rect { w: app.rect.w * 2.0, ..app.rect }, ..app.clone() };
        assert!(locate(pad, &half, &g).is_none());
    }

    #[test]
    fn fractional_scale() {
        // seen on a 1.25 output: 72x24 cells of 22x49 px at scale 2 (11x24.5 points) in an 800x600 window
        let pad = Padding { left: 2.0, right: 2.0, bottom: 2.0 };
        let app = App { rect: Rect { x: 368.0, y: 174.0, w: 800.0, h: 600.0 }, scale: 1.25, ..App::default() };
        let g = Grid { row: 0, col: 22, rows: 24, cols: 72, xpx: 1590, ypx: 1190 };
        let r = locate(pad, &app, &g).unwrap();
        assert_eq!(r, Rect { x: 368.0 + 2.0 + 22.0 * 11.0, y: 174.0 + 600.0 - 2.0 - 595.0, w: 11.0, h: 24.5 });
    }
}
