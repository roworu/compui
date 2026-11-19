//! konsole on linux: like ghostty, the cell comes from the window's client rect and the pty's pixel size,
//! which konsole sets to its text area in logical pixels. the insets come from the default profile
use crate::common::{cell_rect, cells};
use crate::{App, Grid, Handler, Rect};
use std::{env, fs, path::PathBuf};

/// konsole's HIGHLIGHT_SCROLLED_LINES_WIDTH, added to the margin on both sides when the profile has it on
const HIGHLIGHT: f64 = 3.0;
/// widest scrollbar a style is expected to draw
const MAX_SCROLLBAR: f64 = 40.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ScrollBar {
    Left,
    Right,
    Hidden,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Profile {
    pub margin: f64,
    pub highlight: bool,
    pub scrollbar: ScrollBar,
}

/// reads TerminalMargin, HighlightScrolledLines and ScrollBarPosition from a profile, konsole's
/// defaults for what is missing. shortcut: ignores TerminalCenter
pub fn profile(text: &str) -> Profile {
    let mut p = Profile { margin: 1.0, highlight: true, scrollbar: ScrollBar::Right };
    for line in text.lines() {
        let Some((k, v)) = line.split_once('=') else { continue };
        match (k.trim(), v.trim()) {
            ("TerminalMargin", v) => p.margin = v.parse().unwrap_or(p.margin),
            ("HighlightScrolledLines", v) => p.highlight = v == "true",
            ("ScrollBarPosition", "0") => p.scrollbar = ScrollBar::Left,
            ("ScrollBarPosition", "2") => p.scrollbar = ScrollBar::Hidden,
            ("ScrollBarPosition", _) => p.scrollbar = ScrollBar::Right,
            _ => {}
        }
    }
    p
}

/// the default profile's text, from konsolerc's DefaultProfile. shortcut: a session started with another
/// profile gets the default one's insets
fn profile_text() -> String {
    let home = PathBuf::from(env::var_os("HOME").unwrap_or_default());
    let config = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".config"));
    let data = env::var_os("XDG_DATA_HOME").map(PathBuf::from).unwrap_or_else(|| home.join(".local/share"));
    let rc = fs::read_to_string(config.join("konsolerc")).unwrap_or_default();
    let Some(name) = rc.lines().find_map(|l| l.trim().strip_prefix("DefaultProfile=")) else { return String::new() };
    fs::read_to_string(data.join("konsole").join(name.trim())).unwrap_or_default()
}

pub struct Konsole {
    profile: Profile,
}

impl Default for Konsole {
    fn default() -> Self {
        Konsole { profile: profile(&profile_text()) }
    }
}

/// the cell rect: the text area sits margin (+ highlight) in from the left, or past the scrollbar when
/// that is on the left, and margin up from the bottom
pub fn locate(p: Profile, app: &App, g: &Grid) -> Option<Rect> {
    // qt widget sizes are logical pixels already
    let (cw, _, gw, _) = cells(g, 1.0)?;
    let inset = p.margin + if p.highlight { HIGHLIGHT } else { 0.0 };
    // what the window has beside the text area: insets on both sides and the scrollbar
    let extra = app.rect.w - gw;
    let bar = if p.scrollbar == ScrollBar::Hidden { 0.0 } else { MAX_SCROLLBAR };
    // split check: more than that is one pane of several
    if extra < 2.0 * inset - cw || extra > 2.0 * inset + bar + cw {
        return None;
    }
    let left = if p.scrollbar == ScrollBar::Left { extra - inset } else { inset };
    cell_rect(app, g, 1.0, left, p.margin)
}

impl Handler for Konsole {
    fn matches(&self, app: &App) -> bool {
        app.exe == "konsole"
    }
    fn locate(&self, app: &App, g: &Grid) -> Option<Rect> {
        let r = locate(self.profile, app, g);
        crate::log(&format!("konsole window {:?} {:?} grid {}x{} px {}x{} -> {r:?}", app.rect, self.profile, g.cols, g.rows, g.xpx, g.ypx));
        r
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_parses() {
        assert_eq!(profile(""), Profile { margin: 1.0, highlight: true, scrollbar: ScrollBar::Right });
        let text = "[General]\nTerminalMargin=5\n\n[Scrolling]\nHighlightScrolledLines=false\nScrollBarPosition=0\n";
        assert_eq!(profile(text), Profile { margin: 5.0, highlight: false, scrollbar: ScrollBar::Left });
        assert_eq!(profile("ScrollBarPosition=2").scrollbar, ScrollBar::Hidden);
    }

    #[test]
    fn cell_from_bottom() {
        // 80x24 cells of 9x18, 5 px leftover each way, a 14 px scrollbar, menu and tool bars above
        let p = profile("");
        let (gw, gh) = (725.0, 437.0);
        let app = App { rect: Rect { x: 100.0, y: 50.0, w: 4.0 + gw + 4.0 + 14.0, h: 80.0 + 1.0 + gh + 1.0 }, scale: 2.0, ..App::default() };
        let g = Grid { row: 3, col: 4, rows: 24, cols: 80, xpx: 725, ypx: 437 };
        assert_eq!(locate(p, &app, &g), Some(Rect { x: 100.0 + 4.0 + 36.0, y: 50.0 + 81.0 + 54.0, w: 9.0, h: 18.0 }));
        let left = Profile { scrollbar: ScrollBar::Left, ..p };
        assert_eq!(locate(left, &app, &g).unwrap().x, 100.0 + 14.0 + 4.0 + 36.0);
        // twice as wide: a split
        let wide = App { rect: Rect { w: app.rect.w * 2.0, ..app.rect }, ..app.clone() };
        assert!(locate(p, &wide, &g).is_none());
    }
}
