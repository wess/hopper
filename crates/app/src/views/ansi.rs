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

/// A terminal screen: a list of lines plus a cursor.
#[derive(Debug, Default)]
pub struct Screen {
    lines: Vec<Vec<char>>,
    row: usize,
    col: usize,
    /// Bytes carried when an escape sequence spans two writes.
    pending: Vec<u8>,
    /// Cap on retained lines, so a `yes` loop cannot grow without bound.
    max_lines: usize,
}

impl Screen {
    pub fn new(max_lines: usize) -> Self {
        Self {
            lines: vec![Vec::new()],
            row: 0,
            col: 0,
            pending: Vec::new(),
            max_lines: max_lines.max(1),
        }
    }

    fn line_mut(&mut self) -> &mut Vec<char> {
        while self.lines.len() <= self.row {
            self.lines.push(Vec::new());
        }
        &mut self.lines[self.row]
    }

    fn put(&mut self, ch: char) {
        let col = self.col;
        let line = self.line_mut();
        if col < line.len() {
            line[col] = ch; // overwrite, as a terminal does
        } else {
            while line.len() < col {
                line.push(' ');
            }
            line.push(ch);
        }
        self.col += 1;
    }

    fn newline(&mut self) {
        self.row += 1;
        self.col = 0;
        while self.lines.len() <= self.row {
            self.lines.push(Vec::new());
        }
        // Trim from the top once the scrollback cap is hit.
        if self.lines.len() > self.max_lines {
            let excess = self.lines.len() - self.max_lines;
            self.lines.drain(..excess);
            self.row = self.row.saturating_sub(excess);
        }
    }

    fn erase_to_line_end(&mut self) {
        let col = self.col;
        let line = self.line_mut();
        line.truncate(col);
    }

    fn clear_screen(&mut self) {
        self.lines = vec![Vec::new()];
        self.row = 0;
        self.col = 0;
    }

    /// Feed bytes, updating the screen.
    pub fn feed(&mut self, bytes: &[u8]) {
        let mut input = std::mem::take(&mut self.pending);
        input.extend_from_slice(bytes);

        let text = String::from_utf8_lossy(&input);
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '\r' => self.col = 0,
                '\n' => self.newline(),
                '\t' => {
                    // Advance to the next 8-column tab stop.
                    let next = (self.col / 8 + 1) * 8;
                    while self.col < next {
                        self.put(' ');
                    }
                }
                '\x08' => self.col = self.col.saturating_sub(1), // backspace
                '\x07' => {}                                     // bell — ignore
                '\x1b' => {
                    // An escape sequence. If the rest has not arrived yet,
                    // stash it and resume on the next feed.
                    let rest: String = chars.clone().collect();
                    match consume_escape(&rest) {
                        EscapeResult::Consumed { action, len } => {
                            for _ in 0..len {
                                chars.next();
                            }
                            self.apply(action);
                        }
                        EscapeResult::Incomplete => {
                            let mut carry = vec![0x1b];
                            carry.extend_from_slice(rest.as_bytes());
                            self.pending = carry;
                            return;
                        }
                    }
                }
                c if (c as u32) < 0x20 => {} // other control bytes: drop
                c => self.put(c),
            }
        }
    }

    fn apply(&mut self, action: Action) {
        match action {
            Action::EraseToLineEnd => self.erase_to_line_end(),
            Action::ClearScreen => self.clear_screen(),
            Action::CursorCol(n) => self.col = n,
            Action::None => {}
        }
    }

    /// The visible text.
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

enum EscapeResult {
    /// `len` chars after the ESC were consumed.
    Consumed { action: Action, len: usize },
    Incomplete,
}

/// Parse the sequence following an ESC. `rest` is everything after `\x1b`.
fn consume_escape(rest: &str) -> EscapeResult {
    let mut chars = rest.chars();
    match chars.next() {
        None => EscapeResult::Incomplete,
        // CSI: ESC [ ... final-byte.
        Some('[') => {
            let mut params = String::new();
            for (i, c) in chars.enumerate() {
                if c.is_ascii_alphabetic() || c == '~' {
                    // '[' plus the chars through this one.
                    return EscapeResult::Consumed {
                        action: csi_action(&params, c),
                        len: i + 2,
                    };
                }
                params.push(c);
            }
            EscapeResult::Incomplete
        }
        // OSC: ESC ] ... BEL (or ST). Title-setting and the like — drop it.
        Some(']') => {
            for (i, c) in chars.enumerate() {
                if c == '\x07' {
                    return EscapeResult::Consumed {
                        action: Action::None,
                        len: i + 2,
                    };
                }
            }
            EscapeResult::Incomplete
        }
        // A two-char escape (ESC =, ESC >, charset selection, …): drop it.
        Some(_) => EscapeResult::Consumed {
            action: Action::None,
            len: 1,
        },
    }
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
        assert_eq!(
            rendered(b"\x1b]0;my title\x07prompt$ "),
            "prompt$ "
        );
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
