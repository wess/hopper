//! Streaming frames: container logs and interactive exec output.

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum StreamKind {
    Stdout,
    Stderr,
}

impl StreamKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Stdout => "stdout",
            Self::Stderr => "stderr",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogLine {
    pub request_id: String,
    pub text: String,
    pub stream: StreamKind,
    /// Unix millis parsed from the daemon's `timestamps=1` prefix, when asked
    /// for. `None` when timestamps were not requested.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub at: Option<i64>,
}

/// Options for a log stream. The Bun build hardcoded `timestamps: false` and
/// exposed only `tail`, so there was no way to scope a noisy container's
/// history.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogOptions {
    pub tail: u32,
    pub timestamps: bool,
    pub stdout: bool,
    pub stderr: bool,
    /// Unix seconds; only lines at or after this are sent.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub since: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub until: Option<i64>,
    /// Keep the stream open and follow new output.
    pub follow: bool,
}

impl Default for LogOptions {
    fn default() -> Self {
        Self {
            tail: 500,
            timestamps: false,
            stdout: true,
            stderr: true,
            since: None,
            until: None,
            follow: true,
        }
    }
}

/// A chunk of output from an interactive exec session, keyed by session id.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecChunk {
    pub session_id: String,
    pub text: String,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub error: Option<String>,
}
