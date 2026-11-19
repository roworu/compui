//! messages between compui-pty and compui-host, one json object per line
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Cell {
    pub row: u16,
    pub col: u16,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
pub struct Grid {
    pub rows: u16,
    pub cols: u16,
    pub xpx: u16,
    pub ypx: u16,
}

#[derive(Serialize, Deserialize, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "lowercase")]
pub enum Msg {
    Hello { v: u8, session: u32, shell_pid: u32 },
    Line {
        v: u8,
        session: u32,
        seq: u64,
        buffer: String,
        cursor: usize,
        cwd: String,
        path: String,
        history: bool,
        /// recent history lines starting with the text before the cursor, newest first
        #[serde(default)]
        recent: Vec<String>,
        cell: Cell,
        grid: Grid,
    },
    /// popup key taken by the wrapper: up, down, accept or dismiss
    Key { v: u8, session: u32, seq: u64, key: String },
    End { v: u8, session: u32 },
    /// host to wrapper: popup visible, the wrapper starts taking keys
    Shown { v: u8, session: u32 },
    Hidden { v: u8, session: u32 },
    Accept { v: u8, session: u32, seq: u64, start: usize, end: usize, text: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn line_roundtrip() {
        let m = Msg::Line { v: 1, session: 1, seq: 7, buffer: "git ch".into(), cursor: 6, cwd: "/".into(), path: "/bin".into(), history: false, recent: vec!["git checkout main".into()], cell: Cell { row: 12, col: 8 }, grid: Grid { rows: 40, cols: 120, xpx: 0, ypx: 0 } };
        let s = serde_json::to_string(&m).unwrap();
        assert!(s.starts_with(r#"{"type":"line""#));
        assert_eq!(serde_json::from_str::<Msg>(&s).unwrap(), m);
    }
}
