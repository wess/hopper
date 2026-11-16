//! A minimal terminal screen model for the exec pane.
//!
//! Not a full VT100 — but enough that an interactive shell reads cleanly
//! instead of showing raw escape sequences. It keeps a grid of lines and a
//! cursor, and interprets the control codes a shell session actually emits:
//! carriage return (overwrite), backspace, newline, line/screen erase, cursor
//! moves, and SGR (which it strips, since colour is out of scope here).
//!
//! Pure and deterministic, so the parser is tested directly rather than by
//! eyeballing a live shell.

const MAX_COLUMNS: usize = 1024;
const MAX_CELLS: usize = 256 * 1024;
const MAX_ESCAPE_BYTES: usize = 4096;

/// Bounded scrollback and an incremental parser: neither a long line, cursor
/// escape, nor an unfinished title sequence can allocate an unbounded buffer.
#[derive(Debug, Default)]
pub struct Screen {
    lines: std::collections::VecDeque<Vec<char>>,
    col: usize,
    pending: Vec<u8>,
    utf8: Vec<u8>,
    cells: usize,
    max_lines: usize,
}

impl Screen {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: [Vec::new()].into(),
            max_lines: max_lines.clamp(1, 3000),
            ..Self::default()
        }
    }

    fn put(&mut self, ch: char) {
        if self.col >= MAX_COLUMNS {
            return;
        }
        let line = self.lines.back_mut().expect("screen has a line");
        if self.col < line.len() {
            line[self.col] = ch;
        } else {
            self.cells += self.col + 1 - line.len();
            line.resize(self.col, ' ');
            line.push(ch);
        }
        self.col += 1;
        self.trim();
    }

    fn trim(&mut self) {
        while self.lines.len() > self.max_lines || self.cells > MAX_CELLS {
            if self.lines.len() <= 1 {
                break;
            }
            self.cells -= self.lines.pop_front().unwrap().len();
        }
    }

    fn newline(&mut self) {
        self.lines.push_back(Vec::new());
        self.col = 0;
        self.trim();
    }

    fn erase_to_line_end(&mut self) {
        let line = self.lines.back_mut().expect("screen has a line");
        self.cells -= line.len().saturating_sub(self.col);
        line.truncate(self.col);
    }

    fn clear_screen(&mut self) {
        self.lines = [Vec::new()].into();
        self.col = 0;
        self.cells = 0;
    }

    pub fn feed(&mut self, bytes: &[u8]) {
        for &byte in bytes {
            if !self.pending.is_empty() || byte == 0x1b {
                self.escape(byte);
                continue;
            }
            if byte.is_ascii() {
                if !self.utf8.is_empty() {
                    self.utf8.clear();
                    self.put('\u{fffd}');
                }
                match byte {
                    b'\r' => self.col = 0,
                    b'\n' => self.newline(),
                    b'\t' => {
                        let next = ((self.col / 8 + 1) * 8).min(MAX_COLUMNS);
                        while self.col < next {
                            self.put(' ');
                        }
                    }
                    8 => self.col = self.col.saturating_sub(1),
                    0..=31 | 127 => {}
                    _ => self.put(byte as char),
                }
            } else {
                self.utf8.push(byte);
                match std::str::from_utf8(&self.utf8) {
                    Ok(s) => {
                        let ch = s.chars().next().unwrap();
                        self.utf8.clear();
                        self.put(ch);
                    }
                    Err(error) if error.error_len().is_some() || self.utf8.len() >= 4 => {
                        self.utf8.clear();
                        self.put('\u{fffd}');
                    }
                    Err(_) => {}
                }
            }
        }
    }

    fn escape(&mut self, byte: u8) {
        self.pending.push(byte);
        let n = self.pending.len();
        if n > MAX_ESCAPE_BYTES {
            self.pending.clear();
            return;
        }
        if n < 2 {
            return;
        }
        match self.pending[1] {
            b'[' if n >= 3 && (0x40..=0x7e).contains(&byte) => {
                let params = String::from_utf8_lossy(&self.pending[2..n - 1]);
                let action = csi_action(&params, byte as char);
                self.pending.clear();
                self.apply(action);
            }
            b']' if byte == 7 || (n >= 4 && self.pending[n - 2] == 0x1b && byte == b'\\') => {
                self.pending.clear();
            }
            b'[' | b']' => {}
            _ => self.pending.clear(),
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::EraseToLineEnd => self.erase_to_line_end(),
            Action::ClearScreen => self.clear_screen(),
            Action::CursorCol(n) => self.col = n.min(MAX_COLUMNS - 1),
            Action::None => {}
        }
    }

    pub fn render(&self) -> String {
        self.lines
            .iter()
            .map(|line| line.iter().collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

enum Action {
    EraseToLineEnd,
    ClearScreen,
    CursorCol(usize),
    None,
}

fn csi_action(params: &str, final_byte: char) -> Action {
    match final_byte {
        // Erase in line: default (0) and 0 both clear to end.
        'K' => Action::EraseToLineEnd,
        // Erase in display: 2 clears everything.
        'J' if params == "2" || params.is_empty() => Action::ClearScreen,
        // Cursor horizontal absolute: column (1-based) → 0-based.
        'G' => {
            let n = params.parse::<usize>().unwrap_or(1).saturating_sub(1);
            Action::CursorCol(n)
        }
        // Bracketed paste, cursor visibility, other mode toggles: harmless.
        _ => Action::None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hostile_output_cannot_grow_the_screen_or_parser_without_bound() {
        let mut screen = Screen::new(3000);
        screen.feed(&vec![b'x'; 2 * 1024 * 1024]);
        assert_eq!(screen.render().len(), MAX_COLUMNS);
        screen.feed(b"\x1b[18446744073709551615GX");
        assert!(screen.render().len() <= MAX_COLUMNS);
        for _ in 0..400 {
            screen.feed(b"\n");
            screen.feed(&vec![b'x'; MAX_COLUMNS]);
        }
        assert!(screen.cells <= MAX_CELLS);
        screen.feed(b"\x1b]0;");
        screen.feed(&vec![b'x'; 64 * 1024]);
        assert!(screen.pending.len() <= MAX_ESCAPE_BYTES);
        assert!(screen.cells <= MAX_CELLS);
    }

    #[test]
    fn utf8_and_title_terminators_can_cross_chunk_boundaries() {
        let mut screen = Screen::new(3);
        for byte in "héllo →".as_bytes() {
            screen.feed(&[*byte]);
        }
        screen.feed(b"\x1b]title\x1b");
        screen.feed(b"\\ok");
        assert_eq!(screen.render(), "héllo →ok");
    }

    fn rendered(input: &[u8]) -> String {
        let mut s = Screen::new(100);
        s.feed(input);
        s.render()
    }

    #[test]
    fn plain_text_passes_through() {
        assert_eq!(rendered(b"hello world"), "hello world");
    }

    #[test]
    fn a_newline_starts_a_new_line() {
        assert_eq!(rendered(b"a\nb"), "a\nb");
    }

    #[test]
    fn carriage_return_overwrites_from_the_start() {
        // \r returns to column 0 and subsequent text overwrites in place —
        // "done" replaces "load", leaving the rest of "loading..." intact.
        assert_eq!(rendered(b"loading...\rdone"), "doneing...");
        // A full redraw that is at least as long fully replaces the line.
        assert_eq!(rendered(b"50%\r100%"), "100%");
    }

    #[test]
    fn backspace_moves_the_cursor_back() {
        assert_eq!(rendered(b"abc\x08\x08X"), "aXc");
    }

    #[test]
    fn the_bracketed_paste_escape_is_not_shown() {
        // This is the exact sequence that appeared as raw text before.
        assert_eq!(rendered(b"\x1b[?2004hbash-5.1# "), "bash-5.1# ");
    }

    #[test]
    fn sgr_colour_codes_are_stripped() {
        assert_eq!(rendered(b"\x1b[31mred\x1b[0m normal"), "red normal");
    }

    #[test]
    fn erase_to_line_end_clears_the_rest() {
        // Write "abcdef", move cursor to col 3, erase to end.
        assert_eq!(rendered(b"abcdef\x1b[4G\x1b[K"), "abc");
    }

    #[test]
    fn clear_screen_resets_everything() {
        assert_eq!(rendered(b"lots\nof\nstuff\x1b[2Jfresh"), "fresh");
    }

    #[test]
    fn cursor_column_absolute_repositions() {
        // ESC[1G → column 0; overwrite from there.
        assert_eq!(rendered(b"hello\x1b[1GY"), "Yello");
    }

    #[test]
    fn an_osc_title_sequence_is_dropped() {
        assert_eq!(rendered(b"\x1b]0;my title\x07prompt$ "), "prompt$ ");
    }

    #[test]
    fn an_escape_split_across_feeds_is_reassembled() {
        let mut s = Screen::new(100);
        s.feed(b"before\x1b[");
        s.feed(b"Kafter");
        // The CSI K (erase to end) landed at the cursor after "before".
        assert_eq!(s.render(), "beforeafter");
    }

    #[test]
    fn tabs_advance_to_the_next_stop() {
        assert_eq!(rendered(b"a\tb"), "a       b");
    }

    #[test]
    fn scrollback_is_capped() {
        let mut s = Screen::new(3);
        s.feed(b"1\n2\n3\n4\n5");
        // Only the last three lines survive.
        assert_eq!(s.render(), "3\n4\n5");
    }

    #[test]
    fn the_bell_is_silent() {
        assert_eq!(rendered(b"ding\x07dong"), "dingdong");
    }

    #[test]
    fn utf8_survives() {
        assert_eq!(rendered("héllo → wörld".as_bytes()), "héllo → wörld");
    }
}
