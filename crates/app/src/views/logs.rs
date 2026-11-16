//! The retained log tail has independent row, byte, and line-size budgets.

use model::StreamKind;
use std::collections::VecDeque;

const MAX_LINES: usize = 2_000;
const MAX_BYTES: usize = 4 * 1024 * 1024;
const MAX_LINE_BYTES: usize = 16 * 1024;

/// Bound the handoff queue too, and release the original large allocation.
pub fn bounded_line(text: String) -> String {
    if text.len() <= MAX_LINE_BYTES {
        return text;
    }
    let mut end = MAX_LINE_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{} [line truncated]", &text[..end])
}

#[derive(Default)]
pub struct Buffer {
    pub lines: VecDeque<(StreamKind, String)>,
    bytes: usize,
}

impl Buffer {
    pub fn clear(&mut self) {
        self.lines.clear();
        self.bytes = 0;
    }

    pub fn push(&mut self, stream: StreamKind, text: String) {
        let text = bounded_line(text);
        self.bytes += text.len();
        self.lines.push_back((stream, text));
        while self.lines.len() > MAX_LINES || self.bytes > MAX_BYTES {
            if let Some((_, line)) = self.lines.pop_front() {
                self.bytes -= line.len();
            } else {
                break;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retention_obeys_byte_and_row_budgets_and_preserves_the_newest_output() {
        let mut buffer = Buffer::default();
        for _ in 0..MAX_LINES {
            buffer.push(StreamKind::Stdout, "x".repeat(MAX_LINE_BYTES));
        }
        assert!(buffer.bytes <= MAX_BYTES);
        assert!(buffer.lines.len() < MAX_LINES);
        buffer.clear();
        for i in 0..MAX_LINES + 100 {
            buffer.push(StreamKind::Stdout, i.to_string());
        }
        assert_eq!(buffer.lines.len(), MAX_LINES);
        assert_eq!(buffer.lines.front().unwrap().1, "100");
        assert_eq!(buffer.lines.back().unwrap().1, (MAX_LINES + 99).to_string());
        assert_eq!(
            buffer.bytes,
            buffer
                .lines
                .iter()
                .map(|(_, line)| line.len())
                .sum::<usize>()
        );
    }

    #[test]
    fn an_oversized_unicode_line_is_safely_truncated() {
        let mut buffer = Buffer::default();
        buffer.push(StreamKind::Stderr, "€".repeat(MAX_LINE_BYTES));
        let line = &buffer.lines[0].1;
        assert!(line.len() < MAX_LINE_BYTES + 32);
        assert!(line.ends_with(" [line truncated]"));
        assert!(line.starts_with('€'));
        assert!(line.capacity() < MAX_LINE_BYTES * 2);
    }
}
