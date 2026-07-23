//! Docker's stdcopy stream framing.
//!
//! Attached output is multiplexed with an 8-byte header per frame:
//! `[STREAM_TYPE, 0, 0, 0, SIZE(u32 big-endian)]`. Containers created with a
//! TTY are *not* framed — their bytes arrive raw.
//!
//! Two things here are easy to get wrong and both corrupt output:
//!
//! 1. A frame can split across reads, so the framing state must persist
//!    between chunks rather than being re-derived per chunk.
//! 2. A multi-byte UTF-8 character can straddle a frame boundary. Decoding
//!    each payload independently turns those into replacement characters, so
//!    each stream carries its trailing partial sequence forward.
//!
//! Both are pure state machines, so they test without a daemon.

use bytes::Bytes;
use model::StreamKind;

/// Longest UTF-8 sequence; a carry can never exceed this.
const MAX_UTF8: usize = 4;

/// Splits a byte stream into `(stream, payload)` frames.
#[derive(Debug, Default)]
pub struct Frames {
    buf: Vec<u8>,
    /// `None` until the first 8 bytes let us decide. Once set it never flips —
    /// deciding per frame would misread raw TTY output that happens to begin
    /// with a byte sequence shaped like a header.
    framed: Option<bool>,
}

impl Frames {
    pub fn new() -> Self {
        Self::default()
    }

    /// Build a demuxer for a stream whose TTY mode is already known. Skipping
    /// the heuristic is always preferable: the caller learns `tty` from
    /// inspect, and a raw stream can legitimately start with header-shaped
    /// bytes.
    pub fn with_tty(tty: bool) -> Self {
        Self {
            buf: Vec::new(),
            framed: Some(!tty),
        }
    }

    /// A valid header carries a known stream type and three zero pad bytes.
    fn looks_framed(head: &[u8]) -> bool {
        matches!(head[0], 0..=2) && head[1] == 0 && head[2] == 0 && head[3] == 0
    }

    /// Feed a chunk, returning every complete frame it completed.
    pub fn push(&mut self, chunk: &[u8]) -> Vec<(StreamKind, Bytes)> {
        self.buf.extend_from_slice(chunk);
        let mut out = Vec::new();

        loop {
            if self.framed.is_none() {
                if self.buf.len() < 8 {
                    break;
                }
                self.framed = Some(Self::looks_framed(&self.buf[..8]));
            }

            if self.framed == Some(false) {
                if self.buf.is_empty() {
                    break;
                }
                let payload = Bytes::from(std::mem::take(&mut self.buf));
                out.push((StreamKind::Stdout, payload));
                break;
            }

            if self.buf.len() < 8 {
                break;
            }
            let size = u32::from_be_bytes([self.buf[4], self.buf[5], self.buf[6], self.buf[7]]) as usize;
            if self.buf.len() < 8 + size {
                break; // wait for the rest of the frame
            }
            let stream = if self.buf[0] == 2 {
                StreamKind::Stderr
            } else {
                StreamKind::Stdout
            };
            let payload = Bytes::copy_from_slice(&self.buf[8..8 + size]);
            self.buf.drain(..8 + size);
            // A zero-length frame is legal and carries nothing; don't emit it.
            if !payload.is_empty() {
                out.push((stream, payload));
            }
        }

        out
    }

    /// Whatever is left when the stream ends. Unframed output that never
    /// reached 8 bytes still has to be delivered.
    pub fn finish(&mut self) -> Option<(StreamKind, Bytes)> {
        if self.buf.is_empty() {
            return None;
        }
        if self.framed == Some(true) {
            // A truncated frame at end-of-stream: the header can't be trusted,
            // so drop it rather than emit garbage.
            self.buf.clear();
            return None;
        }
        Some((StreamKind::Stdout, Bytes::from(std::mem::take(&mut self.buf))))
    }
}

/// Decodes bytes to UTF-8, carrying an incomplete trailing sequence forward.
#[derive(Debug, Default)]
struct Utf8Carry {
    carry: Vec<u8>,
}

impl Utf8Carry {
    fn decode(&mut self, bytes: &[u8]) -> String {
        let mut input = std::mem::take(&mut self.carry);
        input.extend_from_slice(bytes);

        match std::str::from_utf8(&input) {
            Ok(s) => s.to_string(),
            Err(e) => {
                let good = e.valid_up_to();
                // Keep a trailing *incomplete* sequence for the next chunk;
                // genuinely invalid bytes are replaced rather than hoarded.
                let tail = &input[good..];
                let text = String::from_utf8_lossy(&input[..good]).into_owned();
                if e.error_len().is_none() && tail.len() < MAX_UTF8 {
                    self.carry = tail.to_vec();
                    text
                } else {
                    format!("{text}{}", String::from_utf8_lossy(tail))
                }
            }
        }
    }

    fn flush(&mut self) -> String {
        if self.carry.is_empty() {
            return String::new();
        }
        String::from_utf8_lossy(&std::mem::take(&mut self.carry)).into_owned()
    }
}

/// Frames plus per-stream UTF-8 decoding — what the log and exec readers use.
#[derive(Debug, Default)]
pub struct TextDemux {
    frames: Frames,
    stdout: Utf8Carry,
    stderr: Utf8Carry,
}

impl TextDemux {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_tty(tty: bool) -> Self {
        Self {
            frames: Frames::with_tty(tty),
            ..Default::default()
        }
    }

    pub fn push(&mut self, chunk: &[u8]) -> Vec<(StreamKind, String)> {
        self.frames
            .push(chunk)
            .into_iter()
            .filter_map(|(stream, payload)| {
                let text = match stream {
                    StreamKind::Stdout => self.stdout.decode(&payload),
                    StreamKind::Stderr => self.stderr.decode(&payload),
                };
                (!text.is_empty()).then_some((stream, text))
            })
            .collect()
    }

    /// Flush trailing bytes when the stream ends.
    pub fn finish(&mut self) -> Vec<(StreamKind, String)> {
        let mut out = Vec::new();
        if let Some((stream, payload)) = self.frames.finish() {
            let text = match stream {
                StreamKind::Stdout => self.stdout.decode(&payload),
                StreamKind::Stderr => self.stderr.decode(&payload),
            };
            if !text.is_empty() {
                out.push((stream, text));
            }
        }
        for (stream, carry) in [
            (StreamKind::Stdout, self.stdout.flush()),
            (StreamKind::Stderr, self.stderr.flush()),
        ] {
            if !carry.is_empty() {
                out.push((stream, carry));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn frame(stream: u8, payload: &[u8]) -> Vec<u8> {
        let mut v = vec![stream, 0, 0, 0];
        v.extend_from_slice(&(payload.len() as u32).to_be_bytes());
        v.extend_from_slice(payload);
        v
    }

    fn texts(out: Vec<(StreamKind, String)>) -> Vec<(StreamKind, String)> {
        out
    }

    #[test]
    fn demuxes_stdout_and_stderr_frames() {
        let mut d = TextDemux::new();
        let mut bytes = frame(1, b"out\n");
        bytes.extend(frame(2, b"err\n"));
        assert_eq!(
            texts(d.push(&bytes)),
            vec![
                (StreamKind::Stdout, "out\n".to_string()),
                (StreamKind::Stderr, "err\n".to_string()),
            ]
        );
    }

    #[test]
    fn a_frame_split_across_reads_is_reassembled() {
        let mut d = TextDemux::new();
        let bytes = frame(1, b"hello world");
        // Split mid-payload.
        assert!(d.push(&bytes[..12]).is_empty());
        assert_eq!(
            d.push(&bytes[12..]),
            vec![(StreamKind::Stdout, "hello world".to_string())]
        );
    }

    #[test]
    fn a_header_split_across_reads_is_reassembled() {
        let mut d = TextDemux::new();
        let bytes = frame(2, b"boom");
        // Split inside the 8-byte header.
        assert!(d.push(&bytes[..3]).is_empty());
        assert!(d.push(&bytes[3..6]).is_empty());
        assert_eq!(
            d.push(&bytes[6..]),
            vec![(StreamKind::Stderr, "boom".to_string())]
        );
    }

    #[test]
    fn utf8_split_across_frames_is_not_corrupted() {
        // "é" is 0xC3 0xA9 — put each byte in its own frame.
        let mut d = TextDemux::new();
        let mut bytes = frame(1, &[0xC3]);
        bytes.extend(frame(1, &[0xA9]));
        let out = d.push(&bytes);
        let joined: String = out.into_iter().map(|(_, t)| t).collect();
        assert_eq!(joined, "é");
    }

    #[test]
    fn a_partial_frame_yields_nothing_until_it_completes() {
        let mut d = TextDemux::new();
        let bytes = frame(1, "héllo".as_bytes());
        // Split inside the payload, mid-character. The frame is the unit, so
        // nothing is emitted until all of it has arrived.
        assert!(d.push(&bytes[..10]).is_empty());
        assert_eq!(
            d.push(&bytes[10..]),
            vec![(StreamKind::Stdout, "héllo".to_string())]
        );
    }

    #[test]
    fn utf8_split_across_reads_is_not_corrupted_in_tty_mode() {
        // Raw TTY output has no frames, so a read really can end mid-character
        // and the carry is the only thing preventing a replacement char.
        let mut d = TextDemux::with_tty(true);
        let bytes = "héllo".as_bytes();
        let first = d.push(&bytes[..2]); // 'h' + the lead byte of 'é'
        let second = d.push(&bytes[2..]);
        let joined: String = first
            .into_iter()
            .chain(second)
            .map(|(_, t)| t)
            .collect();
        assert_eq!(joined, "héllo");
    }

    #[test]
    fn tty_output_passes_through_unframed() {
        let mut d = TextDemux::with_tty(true);
        assert_eq!(
            d.push(b"raw terminal bytes"),
            vec![(StreamKind::Stdout, "raw terminal bytes".to_string())]
        );
    }

    #[test]
    fn short_tty_output_is_delivered_on_finish() {
        // Fewer than 8 bytes, so the heuristic never fires during push.
        let mut d = TextDemux::new();
        assert!(d.push(b"hi").is_empty());
        assert_eq!(d.finish(), vec![(StreamKind::Stdout, "hi".to_string())]);
    }

    #[test]
    fn unframed_output_is_detected_and_stays_unframed() {
        let mut d = TextDemux::new();
        // Plain text: first byte 'p' is not a valid stream type.
        assert_eq!(
            d.push(b"plain output that is long enough"),
            vec![(
                StreamKind::Stdout,
                "plain output that is long enough".to_string()
            )]
        );
        // A later chunk that happens to look like a header stays raw.
        assert_eq!(
            d.push(&[1, 0, 0, 0, 0, 0, 0, 4, b'x']),
            vec![(StreamKind::Stdout, "\u{1}\0\0\0\0\0\0\u{4}x".to_string())]
        );
    }

    #[test]
    fn zero_length_frames_emit_nothing() {
        let mut d = TextDemux::new();
        let mut bytes = frame(1, b"");
        bytes.extend(frame(1, b"after"));
        assert_eq!(
            d.push(&bytes),
            vec![(StreamKind::Stdout, "after".to_string())]
        );
    }

    #[test]
    fn many_frames_in_one_read_all_come_out() {
        let mut d = TextDemux::new();
        let mut bytes = Vec::new();
        for i in 0..100 {
            bytes.extend(frame(1, format!("line {i}\n").as_bytes()));
        }
        assert_eq!(d.push(&bytes).len(), 100);
    }

    #[test]
    fn a_truncated_trailing_frame_is_dropped_rather_than_emitted_as_garbage() {
        let mut d = TextDemux::new();
        let bytes = frame(1, b"complete");
        assert_eq!(d.push(&bytes).len(), 1);
        // Header promising 50 bytes, only 2 delivered, then the stream dies.
        d.push(&[1, 0, 0, 0, 0, 0, 0, 50, b'a', b'b']);
        assert!(d.finish().is_empty());
    }
}
