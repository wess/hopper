//! Container log streaming.
//!
//! Output arrives stdcopy-framed unless the container has a TTY, so the reader
//! asks the daemon whether it does rather than guessing from the bytes.

use crate::client::{Client, Req};
use crate::containers;
use crate::demux::TextDemux;
use crate::error::Result;
use model::{LogLine, LogOptions, StreamKind};

/// Split the RFC3339 timestamp the daemon prefixes when `timestamps=1`.
///
/// Returns the unix-millis timestamp and the remaining text. A line without a
/// parseable prefix comes back untouched — an odd line must not be dropped or
/// have its first word eaten.
pub fn split_timestamp(line: &str) -> (Option<i64>, &str) {
    let Some(space) = line.find(' ') else {
        return (None, line);
    };
    let (stamp, rest) = line.split_at(space);
    match chrono::DateTime::parse_from_rfc3339(stamp) {
        Ok(dt) => (Some(dt.timestamp_millis()), &rest[1..]),
        Err(_) => (None, line),
    }
}

fn build_request(id: &str, opts: &LogOptions) -> Req {
    let req = Req::get(format!("/containers/{id}/logs"))
        .flag("follow", opts.follow)
        .flag("stdout", opts.stdout)
        .flag("stderr", opts.stderr)
        .flag("timestamps", opts.timestamps)
        .query("tail", opts.tail)
        .query_opt("since", opts.since)
        .query_opt("until", opts.until);
    if opts.follow {
        req.no_timeout()
    } else {
        req
    }
}

/// Reassembles whole lines from frames that are not line-aligned.
///
/// A frame can end mid-line, and stdout and stderr interleave, so each stream
/// carries its own trailing fragment forward. Getting this wrong splits single
/// log lines in half in the viewer.
#[derive(Debug, Default)]
pub struct Lines {
    partial: Vec<(StreamKind, String)>,
}

impl Lines {
    pub fn new() -> Self {
        Self::default()
    }

    /// Feed one frame, returning the lines it completed.
    pub fn push(&mut self, stream: StreamKind, text: &str) -> Vec<(StreamKind, String)> {
        let carried = match self.partial.iter().position(|(s, _)| *s == stream) {
            Some(i) => {
                let (_, prev) = self.partial.remove(i);
                format!("{prev}{text}")
            }
            None => text.to_string(),
        };

        let mut parts: Vec<&str> = carried.split('\n').collect();
        // `split` always yields a trailing element: empty when the text ended
        // on a newline, otherwise the incomplete tail.
        let tail = parts.pop().unwrap_or_default().to_string();
        let out = parts
            .into_iter()
            .map(|l| (stream, l.trim_end_matches('\r').to_string()))
            .collect();
        if !tail.is_empty() {
            self.partial.push((stream, tail));
        }
        out
    }

    /// Whatever never received a trailing newline still belongs to the user.
    pub fn finish(&mut self) -> Vec<(StreamKind, String)> {
        std::mem::take(&mut self.partial)
            .into_iter()
            .filter(|(_, t)| !t.is_empty())
            .map(|(s, t)| (s, t.trim_end_matches('\r').to_string()))
            .collect()
    }
}

/// Stream a container's logs, invoking `on_line` per line.
///
/// Returning `false` stops the stream; dropping the future does the same.
pub async fn stream<F>(
    client: &Client,
    request_id: &str,
    id: &str,
    opts: &LogOptions,
    mut on_line: F,
) -> Result<()>
where
    F: FnMut(LogLine) -> bool,
{
    // Knowing the TTY mode up front beats inferring it: raw output can
    // legitimately begin with header-shaped bytes.
    let tty = containers::has_tty(client, id).await.unwrap_or(false);
    let mut demux = TextDemux::with_tty(tty);
    let mut lines = Lines::new();
    let timestamps = opts.timestamps;
    let mut keep = true;

    let emit = |stream: StreamKind, text: &str, on_line: &mut F| -> bool {
        let (at, body) = if timestamps {
            split_timestamp(text)
        } else {
            (None, text)
        };
        on_line(LogLine {
            request_id: request_id.to_string(),
            text: body.to_string(),
            stream,
            at,
        })
    };

    client
        .stream(build_request(id, opts), |chunk| {
            for (stream, text) in demux.push(&chunk) {
                for (stream, line) in lines.push(stream, &text) {
                    if !emit(stream, &line, &mut on_line) {
                        keep = false;
                        return false;
                    }
                }
            }
            true
        })
        .await?;

    if keep {
        for (stream, text) in demux.finish() {
            for (stream, line) in lines.push(stream, &text) {
                if !emit(stream, &line, &mut on_line) {
                    return Ok(());
                }
            }
        }
        for (stream, line) in lines.finish() {
            if !emit(stream, &line, &mut on_line) {
                break;
            }
        }
    }
    Ok(())
}

/// Fetch a bounded slice of logs without following. Used by the MCP server and
/// the diagnostics bundle.
pub async fn snapshot(client: &Client, id: &str, tail: u32) -> Result<Vec<LogLine>> {
    let opts = LogOptions {
        tail,
        follow: false,
        ..Default::default()
    };
    let mut out = Vec::new();
    stream(client, "snapshot", id, &opts, |line| {
        out.push(line);
        true
    })
    .await?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamps_are_split_off_the_front_of_a_line() {
        let (at, text) = split_timestamp("2026-01-01T00:00:00.000000000Z hello world");
        assert!(at.is_some());
        assert_eq!(text, "hello world");
    }

    #[test]
    fn a_line_without_a_timestamp_survives_intact() {
        let (at, text) = split_timestamp("plain log line");
        assert!(at.is_none());
        assert_eq!(text, "plain log line");
    }

    #[test]
    fn a_line_with_no_space_is_not_truncated() {
        let (at, text) = split_timestamp("singleword");
        assert!(at.is_none());
        assert_eq!(text, "singleword");
    }

    #[test]
    fn a_word_that_is_not_a_timestamp_is_not_eaten() {
        let (at, text) = split_timestamp("ERROR something failed");
        assert!(at.is_none());
        assert_eq!(text, "ERROR something failed");
    }

    #[test]
    fn complete_lines_come_out_whole() {
        let mut l = Lines::new();
        assert_eq!(
            l.push(StreamKind::Stdout, "one\ntwo\n"),
            vec![
                (StreamKind::Stdout, "one".to_string()),
                (StreamKind::Stdout, "two".to_string()),
            ]
        );
        assert!(l.finish().is_empty());
    }

    #[test]
    fn a_line_split_across_frames_is_rejoined_not_halved() {
        let mut l = Lines::new();
        assert!(l.push(StreamKind::Stdout, "hello ").is_empty());
        assert_eq!(
            l.push(StreamKind::Stdout, "world\n"),
            vec![(StreamKind::Stdout, "hello world".to_string())]
        );
    }

    #[test]
    fn stdout_and_stderr_fragments_do_not_contaminate_each_other() {
        let mut l = Lines::new();
        assert!(l.push(StreamKind::Stdout, "out-part").is_empty());
        assert!(l.push(StreamKind::Stderr, "err-part").is_empty());
        assert_eq!(
            l.push(StreamKind::Stdout, "-done\n"),
            vec![(StreamKind::Stdout, "out-part-done".to_string())]
        );
        assert_eq!(
            l.push(StreamKind::Stderr, "-done\n"),
            vec![(StreamKind::Stderr, "err-part-done".to_string())]
        );
    }

    #[test]
    fn a_trailing_line_without_a_newline_is_still_delivered() {
        let mut l = Lines::new();
        assert!(l.push(StreamKind::Stdout, "no trailing newline").is_empty());
        assert_eq!(
            l.finish(),
            vec![(StreamKind::Stdout, "no trailing newline".to_string())]
        );
    }

    #[test]
    fn carriage_returns_are_trimmed_so_windows_logs_render_cleanly() {
        let mut l = Lines::new();
        assert_eq!(
            l.push(StreamKind::Stdout, "line\r\n"),
            vec![(StreamKind::Stdout, "line".to_string())]
        );
    }

    #[test]
    fn blank_lines_are_preserved_rather_than_swallowed() {
        let mut l = Lines::new();
        assert_eq!(
            l.push(StreamKind::Stdout, "a\n\nb\n"),
            vec![
                (StreamKind::Stdout, "a".to_string()),
                (StreamKind::Stdout, String::new()),
                (StreamKind::Stdout, "b".to_string()),
            ]
        );
    }
}
