//! Interactive exec.
//!
//! `POST /exec/{id}/start` upgrades the connection to a raw bidirectional
//! stream. Once hijacked, the socket carries the container's stdout (and, when
//! no TTY was allocated, stdcopy-framed stderr) in one direction and keystrokes
//! in the other. Nothing above this layer sees HTTP again.

use crate::client::{Client, Req};
use crate::demux::TextDemux;
use crate::error::{DockerError, Result};
use serde::Deserialize;
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::mpsc;

/// How the shell for a session was chosen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shell {
    pub argv: Vec<String>,
}

/// Candidate shells, best first.
///
/// The TypeScript build hardcoded `/bin/sh`, which fails outright on
/// distroless images and gives a worse experience than bash where bash exists.
pub const SHELL_CANDIDATES: [&str; 4] = ["/bin/bash", "/bin/sh", "/bin/ash", "/busybox/sh"];

/// Build the command for a session: an explicit request wins, otherwise probe
/// for the first shell that exists.
pub fn shell_command(requested: Option<&str>) -> Vec<String> {
    if let Some(cmd) = requested.map(str::trim).filter(|c| !c.is_empty()) {
        return crate::containers::split_command(cmd);
    }
    // `command -v` in a POSIX shell reports the first candidate that exists;
    // falling back to sh keeps the old behavior when nothing else is present.
    let probe = SHELL_CANDIDATES
        .iter()
        .map(|s| format!("[ -x {s} ] && exec {s}"))
        .collect::<Vec<_>>()
        .join("; ");
    vec!["/bin/sh".into(), "-c".into(), format!("{probe}; exec /bin/sh")]
}

#[derive(Debug, Deserialize, Default)]
struct Created {
    #[serde(rename = "Id")]
    id: String,
}

/// A live exec session. Dropping it closes the socket, which ends the session.
pub struct Session {
    pub id: String,
    input: mpsc::UnboundedSender<Vec<u8>>,
    client: Client,
}

impl Session {
    /// Send keystrokes to the container.
    pub fn write(&self, bytes: impl Into<Vec<u8>>) -> bool {
        self.input.send(bytes.into()).is_ok()
    }

    /// Tell the daemon the terminal was resized, so full-screen programs
    /// re-flow instead of drawing to the wrong width.
    pub async fn resize(&self, cols: u16, rows: u16) -> Result<()> {
        self.client
            .action(
                Req::post(format!("/exec/{}/resize", self.id))
                    .query("w", cols)
                    .query("h", rows),
            )
            .await
    }
}

/// Start an interactive exec session.
///
/// `on_output` receives decoded text as it arrives. The returned [`Session`]
/// writes keystrokes back. The session ends when the container exits, the
/// callback returns `false`, or the session is dropped.
pub async fn start<F>(
    client: &Client,
    container: &str,
    shell: Option<&str>,
    tty: bool,
    mut on_output: F,
) -> Result<Session>
where
    F: FnMut(String) -> bool + Send + 'static,
{
    let cmd = shell_command(shell);
    let created: Created = client
        .json(
            Req::post(format!("/containers/{container}/exec")).json_body(json!({
                "AttachStdin": true,
                "AttachStdout": true,
                "AttachStderr": true,
                "Tty": tty,
                "Cmd": cmd,
            })),
        )
        .await?;

    if created.id.is_empty() {
        return Err(DockerError::api(
            500,
            "The daemon did not return an exec id.",
        ));
    }

    let upgraded = client
        .upgrade(
            // Ask for a real protocol upgrade so the daemon replies 101 and
            // hyper hands back the hijacked stream. Without these headers
            // Docker returns 200 and takes over the connection in a way hyper
            // reports as "no upgrade available".
            Req::post(format!("/exec/{}/start", created.id))
                .json_body(json!({ "Detach": false, "Tty": tty }))
                .header("Connection", "Upgrade")
                .header("Upgrade", "tcp")
                .no_timeout(),
        )
        .await?;

    let (tx, mut rx) = mpsc::unbounded_channel::<Vec<u8>>();
    let (mut reader, mut writer) = tokio::io::split(upgraded);

    // Pump keystrokes in.
    tokio::spawn(async move {
        while let Some(bytes) = rx.recv().await {
            if writer.write_all(&bytes).await.is_err() {
                break;
            }
            let _ = writer.flush().await;
        }
    });

    // Pump output out. A TTY session is unframed; without one the daemon
    // stdcopy-frames stdout and stderr.
    tokio::spawn(async move {
        let mut demux = TextDemux::with_tty(tty);
        let mut buf = vec![0u8; 8192];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => {
                    for (_, text) in demux.push(&buf[..n]) {
                        if !on_output(text) {
                            return;
                        }
                    }
                }
            }
        }
        for (_, text) in demux.finish() {
            if !on_output(text) {
                return;
            }
        }
    });

    Ok(Session {
        id: created.id,
        input: tx,
        client: client.clone(),
    })
}

/// Run a command to completion and collect its output. Used by the MCP server
/// and by helpers such as volume browsing.
pub async fn run_once(
    client: &Client,
    container: &str,
    argv: &[String],
) -> Result<(String, i64)> {
    let created: Created = client
        .json(
            Req::post(format!("/containers/{container}/exec")).json_body(json!({
                "AttachStdout": true,
                "AttachStderr": true,
                "Tty": false,
                "Cmd": argv,
            })),
        )
        .await?;

    let mut out = String::new();
    let mut demux = TextDemux::with_tty(false);
    client
        .stream(
            Req::post(format!("/exec/{}/start", created.id))
                .json_body(json!({ "Detach": false, "Tty": false }))
                .no_timeout(),
            |chunk| {
                for (_, text) in demux.push(&chunk) {
                    out.push_str(&text);
                }
                true
            },
        )
        .await?;
    for (_, text) in demux.finish() {
        out.push_str(&text);
    }

    #[derive(Deserialize, Default)]
    struct Inspect {
        #[serde(rename = "ExitCode")]
        exit_code: Option<i64>,
    }
    let info: Inspect = client
        .json(Req::get(format!("/exec/{}/json", created.id)))
        .await
        .unwrap_or_default();

    Ok((out, info.exit_code.unwrap_or(0)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_explicit_shell_request_is_used_verbatim() {
        assert_eq!(shell_command(Some("/bin/zsh")), vec!["/bin/zsh"]);
    }

    #[test]
    fn an_explicit_request_is_split_like_a_shell_would() {
        assert_eq!(
            shell_command(Some(r#"sh -c "echo hi""#)),
            vec!["sh", "-c", "echo hi"]
        );
    }

    #[test]
    fn a_blank_request_falls_back_to_probing() {
        let cmd = shell_command(Some("   "));
        assert_eq!(cmd[0], "/bin/sh");
        assert_eq!(cmd[1], "-c");
    }

    #[test]
    fn the_probe_tries_every_candidate_and_still_ends_at_sh() {
        let cmd = shell_command(None);
        let script = &cmd[2];
        for candidate in SHELL_CANDIDATES {
            assert!(
                script.contains(candidate),
                "probe should try {candidate}"
            );
        }
        // A distroless image with none of them still gets a final attempt
        // rather than an empty command.
        assert!(script.ends_with("exec /bin/sh"));
    }

    #[test]
    fn bash_is_preferred_over_sh_when_both_exist() {
        let script = shell_command(None).remove(2);
        let bash = script.find("/bin/bash").unwrap();
        let sh = script.find("[ -x /bin/sh ]").unwrap();
        assert!(bash < sh, "bash should be probed before sh");
    }
}
