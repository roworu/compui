//! the terminal's cursor cell and alt screen, tracked from its output. the wrapper needs no more,
//! and keeping vt100's cell grid made parsing the slowest part of passing output through. this
//! follows vt100 0.18's cursor rules (the tests check it against vt100), gaps included: no ESC D or
//! ESC E, tab stops every 8, no DECAWM. one shortcut: U+FE0F widens a char only right after it
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

/// the cursor state of one screen; the main and alt screens each have their own
#[derive(Clone, Copy, Default)]
struct Grid {
    row: u16,
    col: u16,
    saved: (u16, u16),
    top: u16,
    bottom: u16,
    origin: bool,
    saved_origin: bool,
    /// the main screen counts rows scrolled off its top, pulled back down when it grows
    scrollback: bool,
    scrolled: usize,
    /// the alt screen has no rows until first used, so a resize doesn't move its cursor
    allocated: bool,
}

impl Grid {
    fn in_region(&self) -> bool {
        self.top <= self.row && self.row <= self.bottom
    }

    fn scroll_up(&mut self, n: u16) {
        if self.scrollback && self.top == 0 {
            self.scrolled += usize::from(n.min(self.bottom - self.top + 1));
        }
    }

    fn resize(&mut self, old_rows: u16, rows: u16, cols: u16) {
        self.top = 0;
        let diff = old_rows.abs_diff(rows);
        if rows < old_rows {
            let pushed = if self.allocated { diff.saturating_sub(old_rows.saturating_sub(self.row + 1)) } else { 0 };
            if self.allocated && self.scrollback {
                self.scrolled += usize::from(pushed);
            }
            self.row = self.row.saturating_sub(pushed);
            self.saved.0 = self.saved.0.saturating_sub(pushed).min(rows - 1);
        } else if self.allocated && self.scrollback {
            let pulled = u16::try_from(self.scrolled).unwrap_or(u16::MAX).min(diff);
            self.scrolled -= usize::from(pulled);
            self.row += pulled;
            self.saved.0 += pulled;
        }
        self.col = self.col.min(cols - 1);
        self.saved.1 = self.saved.1.min(cols - 1);
        self.bottom = rows - 1;
    }
}

struct Screen {
    rows: u16,
    cols: u16,
    main: Grid,
    alt: Grid,
    alt_on: bool,
    /// for CSI b (repeat)
    last_char: Option<char>,
    /// a narrow char just printed (row, col, char), which a U+FE0F right after makes wide
    last_cell: Option<(u16, u16, char)>,
}

impl Screen {
    fn new(rows: u16, cols: u16) -> Screen {
        let g = Grid { bottom: rows - 1, ..Grid::default() };
        Screen { rows, cols, main: Grid { scrollback: true, allocated: true, ..g }, alt: g, alt_on: false, last_char: None, last_cell: None }
    }

    fn g(&mut self) -> &mut Grid {
        if self.alt_on { &mut self.alt } else { &mut self.main }
    }

    /// down `n` rows, stopping at the region's bottom if inside it; a line feed scrolls there
    fn down(&mut self, n: u16, feed: bool) {
        let rows = self.rows;
        let g = self.g();
        let in_region = g.in_region();
        let bottom = if in_region { g.bottom } else { rows - 1 };
        g.row = g.row.saturating_add(n);
        if g.row > bottom {
            let lines = g.row - bottom;
            g.row = bottom;
            if feed && in_region {
                g.scroll_up(lines);
            }
        }
    }

    fn up(&mut self, n: u16) {
        let g = self.g();
        let in_region = g.in_region();
        g.row = g.row.saturating_sub(n);
        if in_region && g.row < g.top {
            g.row = g.top;
        }
    }

    fn set_col(&mut self, col: u16) {
        let cols = self.cols;
        self.g().col = col.min(cols - 1);
    }

    fn set_pos(&mut self, row: u16, col: u16) {
        let (rows, cols) = (self.rows, self.cols);
        let g = self.g();
        let bottom = if g.origin { g.bottom } else { rows - 1 };
        let row = if g.origin { row.saturating_add(g.top) } else { row };
        g.row = row.min(bottom);
        g.col = col.min(cols - 1);
    }

    fn save(&mut self) {
        let g = self.g();
        g.saved = (g.row, g.col);
        g.saved_origin = g.origin;
    }

    fn restore(&mut self) {
        let g = self.g();
        (g.row, g.col) = g.saved;
        g.origin = g.saved_origin;
    }

    fn text(&mut self, c: char) {
        let width = match c {
            ' '..='~' => 1,
            _ => match c.width() {
                None if u32::from(c) < 256 => return,
                w => w.unwrap_or(1).min(2) as u16,
            },
        };
        let cols = self.cols;
        if width > cols {
            return;
        }
        let g = self.g();
        let (row, col) = (g.row, g.col);
        if width == 0 {
            if col > 0 && c == '\u{FE0F}' && matches!(self.last_cell, Some((r, c0, b)) if r == row && c0 + 1 == col && format!("{b}\u{FE0F}").width() > 1) {
                self.last_cell = None;
                if col > cols - 1 {
                    self.g().col = 0;
                    self.down(1, true);
                    self.g().col = 2;
                } else {
                    self.g().col += 1;
                }
            }
            return;
        }
        if col > cols - width {
            g.col = 0;
            self.down(1, true);
        }
        let g = self.g();
        let (row, col) = (g.row, g.col);
        g.col += width;
        self.last_cell = (width == 1).then_some((row, col, c));
        self.last_char = Some(c);
    }
}

/// the first param, `default` if missing or 0
fn param(params: &vte::Params, i: usize, default: u16) -> u16 {
    match params.iter().nth(i).map_or(0, |p| p.first().copied().unwrap_or(0)) {
        0 => default,
        n => n,
    }
}

impl vte::Perform for Screen {
    fn print(&mut self, c: char) {
        if c != '\u{fffd}' && !('\u{80}'..'\u{a0}').contains(&c) {
            self.text(c);
        }
    }

    fn execute(&mut self, b: u8) {
        self.last_cell = None;
        match b {
            8 => {
                let g = self.g();
                g.col = g.col.saturating_sub(1);
            }
            9 => {
                let cols = self.cols;
                let g = self.g();
                g.col = (g.col - g.col % 8).saturating_add(8).min(cols - 1);
            }
            10..=12 => self.down(1, true),
            13 => self.g().col = 0,
            _ => {}
        }
    }

    fn esc_dispatch(&mut self, intermediates: &[u8], _ignore: bool, b: u8) {
        self.last_cell = None;
        if !intermediates.is_empty() {
            return;
        }
        match b {
            b'7' => self.save(),
            b'8' => self.restore(),
            b'M' => self.up(1),
            b'c' => *self = Screen::new(self.rows, self.cols),
            _ => {}
        }
    }

    fn csi_dispatch(&mut self, params: &vte::Params, intermediates: &[u8], _ignore: bool, c: char) {
        self.last_cell = None;
        let n = param(params, 0, 1);
        match (intermediates.first(), c) {
            (None, 'A') => self.up(n),
            (None, 'B') => self.down(n, false),
            (None, 'C') => {
                let cols = self.cols;
                let g = self.g();
                g.col = g.col.saturating_add(n).min(cols - 1);
            }
            (None, 'D') => {
                let g = self.g();
                g.col = g.col.saturating_sub(n);
            }
            (None, 'E') => {
                self.g().col = 0;
                self.down(n, false);
            }
            (None, 'F') => {
                self.g().col = 0;
                self.up(n);
            }
            (None, 'G' | '`') => self.set_col(n - 1),
            (None, 'H' | 'f') => self.set_pos(n - 1, param(params, 1, 1) - 1),
            (None, 'S') => self.g().scroll_up(n),
            // erase scrollback: nothing left to pull down on a resize
            (None | Some(b'?'), 'J') if param(params, 0, 0) == 3 => self.g().scrolled = 0,
            (None, 'b') => {
                if let Some(c) = self.last_char {
                    for _ in 0..n {
                        self.text(c);
                    }
                }
            }
            (None, 'd') => {
                let rows = self.rows;
                self.g().row = (n - 1).min(rows - 1);
            }
            (None, 'r') => {
                let rows = self.rows;
                let (top, bottom) = (n - 1, (param(params, 1, rows) - 1).min(rows - 1));
                let g = self.g();
                (g.top, g.bottom) = if top < bottom { (top, bottom) } else { (0, rows - 1) };
                (g.row, g.col) = (g.top, 0);
            }
            (Some(b'?'), 'h') => {
                for p in params {
                    match p {
                        [6] => {
                            self.g().origin = true;
                            self.set_pos(0, 0);
                        }
                        [47] => self.enter_alt(),
                        [1049] => {
                            self.save();
                            self.alt = Grid { allocated: self.alt.allocated, bottom: self.rows - 1, ..Grid::default() };
                            self.enter_alt();
                        }
                        _ => {}
                    }
                }
            }
            (Some(b'?'), 'l') => {
                for p in params {
                    match p {
                        [6] => {
                            self.g().origin = false;
                            self.set_pos(0, 0);
                        }
                        [47] => self.alt_on = false,
                        [1049] => {
                            self.alt_on = false;
                            self.restore();
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn osc_dispatch(&mut self, _params: &[&[u8]], _bell_terminated: bool) {
        self.last_cell = None;
    }

    fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], _ignore: bool, _action: char) {
        self.last_cell = None;
    }
}

impl Screen {
    fn enter_alt(&mut self) {
        self.alt.allocated = true;
        self.alt_on = true;
    }
}

pub struct Tracker {
    parser: vte::Parser,
    screen: Screen,
}

impl Tracker {
    /// a tracker for a screen of `rows` x `cols`, cursor at the top left
    pub fn new(rows: u16, cols: u16) -> Tracker {
        Tracker { parser: vte::Parser::new(), screen: Screen::new(rows.max(1), cols.max(1)) }
    }

    /// follows the terminal through `bytes` of its output
    pub fn process(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.screen, bytes);
    }

    /// (row, col), 0-based; col is one past the last column while a wrap is pending
    pub fn cursor_position(&self) -> (u16, u16) {
        let g = if self.screen.alt_on { &self.screen.alt } else { &self.screen.main };
        (g.row, g.col)
    }

    /// whether a full-screen program has the alt screen
    pub fn alternate_screen(&self) -> bool {
        self.screen.alt_on
    }

    /// resizes like vt100 (no reflow; the cursor keeps its distance from the bottom)
    pub fn set_size(&mut self, rows: u16, cols: u16) {
        let (rows, cols) = (rows.max(1), cols.max(1));
        let s = &mut self.screen;
        if (rows, cols) == (s.rows, s.cols) {
            return;
        }
        s.main.resize(s.rows, rows, cols);
        s.alt.resize(s.rows, rows, cols);
        (s.rows, s.cols) = (rows, cols);
    }
}

#[cfg(test)]
mod tests {
    use super::Tracker;
    use std::num::NonZeroU16 as NZ;

    struct Both {
        ours: Tracker,
        theirs: vt100::Parser,
        log: Vec<String>,
    }

    impl Both {
        fn new(rows: u16, cols: u16) -> Both {
            Both { ours: Tracker::new(rows, cols), theirs: vt100::Parser::new(NZ::new(rows).unwrap(), NZ::new(cols).unwrap(), 0), log: Vec::new() }
        }

        /// false if vt100 panicked (some wide char edits make it unwrap a missing cell)
        fn feed(&mut self, b: &[u8]) -> bool {
            self.log.push(format!("{:?}", String::from_utf8_lossy(b)));
            self.ours.process(b);
            let theirs = &mut self.theirs;
            if std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| theirs.process(b))).is_err() {
                eprintln!("vt100 panicked after {}", self.log.join(" "));
                return false;
            }
            self.check();
            true
        }

        fn resize(&mut self, rows: u16, cols: u16) {
            self.log.push(format!("resize {rows}x{cols}"));
            self.ours.set_size(rows, cols);
            self.theirs.screen_mut().set_size(NZ::new(rows).unwrap(), NZ::new(cols).unwrap());
            self.check();
        }

        fn check(&self) {
            let s = self.theirs.screen();
            let want = (s.cursor_position(), s.alternate_screen());
            let got = (self.ours.cursor_position(), self.ours.alternate_screen());
            assert_eq!(got, want, "after {}", self.log.join(" "));
        }
    }

    #[test]
    fn shell_like_output() {
        let mut t = Both::new(24, 80);
        for b in [
            &b"\x1b[1;1H"[..],
            b"\x1b[?2004hbench$ git ch",
            b"\x08\x08\x1b[K",
            b"\r\x1b[2Kbench$ ls -la\x1b[7D",
            b"\r\n",
            &b"line\r\n".repeat(40),
            b"\ttab\tstops\t",
            "漢字 wide ☺\u{FE0F} e\u{301}\r\n".as_bytes(),
            b"\x1b[?1049h\x1b[H\x1b[2Jvim\x1b[10;20H\x1b[?1049l",
            b"\x1b7\x1b[5;5H\x1b8",
            b"\x1b[5;10r\x1b[?6h\x1b[3;3H\x1b[20B\n\n\x1b[?6l\x1b[r",
            b"x\x1b[5b\xff\xfe\x1bc",
        ] {
            t.feed(b);
        }
    }

    #[test]
    fn wraps_and_resizes() {
        let mut t = Both::new(5, 10);
        t.feed(&b"0123456789".repeat(7));
        t.feed(b"\x08ab");
        t.resize(3, 6);
        t.feed(b"\r\nmore\r\nlines\r\n");
        t.resize(8, 12);
        t.feed("\x1b[1;12H☺\u{FE0F}漢".as_bytes());
        t.resize(8, 12);
        t.resize(2, 1);
        t.feed("漢x".as_bytes());
    }

    /// random escape soup, small screens and resizes, checked against vt100 after every step
    #[test]
    fn matches_vt100() {
        let mut seed = 0x2545F4914F6CDD1Du64;
        let mut rand = move |n: u64| {
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            seed % n
        };
        let csi = b"ABCDEFGHJKLMPSTXdfrb`@";
        for _ in 0..1500 {
            let mut t = Both::new(1 + rand(12) as u16, 1 + rand(20) as u16);
            for _ in 0..200 {
                let mut b = Vec::new();
                match rand(16) {
                    0..=3 => b.extend((0..1 + rand(30)).map(|_| b' ' + rand(95) as u8)),
                    4 => b.extend_from_slice(["漢", "☺\u{FE0F}", "a\u{FE0F}", "e\u{301}", "e\u{301}\u{FE0F}", "\u{200B}", "😀"][rand(7) as usize].as_bytes()),
                    5 => b.push([b'\r', b'\n', b'\x08', b'\t', b'\x0b', b'\x0c', b'\x07', 0xff][rand(8) as usize]),
                    6..=8 => {
                        b.extend_from_slice(b"\x1b[");
                        for i in 0..rand(3) {
                            if i > 0 {
                                b.push(b';');
                            }
                            b.extend_from_slice(rand(25).to_string().as_bytes());
                        }
                        b.push(csi[rand(csi.len() as u64) as usize]);
                    }
                    9 => b.extend_from_slice([&b"\x1b7"[..], b"\x1b8", b"\x1bM", b"\x1bc", b"\x1bD", b"\x1b]0;title\x07"][rand(6) as usize]),
                    10 => b.extend_from_slice(format!("\x1b[?{}{}", [6, 47, 1049, 25][rand(4) as usize], ["h", "l"][rand(2) as usize]).as_bytes()),
                    11 => {
                        t.resize(1 + rand(12) as u16, 1 + rand(20) as u16);
                        continue;
                    }
                    _ => b.extend_from_slice(b"\r\n"),
                }
                if !t.feed(&b) {
                    break;
                }
            }
        }
    }
}
