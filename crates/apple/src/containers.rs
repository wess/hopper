//! Containers, through the `container` CLI.

use docker::{DockerError, Result};
use model::{Container, InspectResult, PruneReport, RunInput};

use crate::cli::Cli;
use crate::wire::ManagedContainer;

pub async fn list(cli: &Cli, all: bool) -> Result<Vec<Container>> {
    let mut args = vec!["list"];
    if all {
        args.push("--all");
    }
    let raw: Vec<ManagedContainer> = cli.json(&args).await?;
    Ok(raw.into_iter().map(ManagedContainer::into_model).collect())
}

pub async fn inspect(cli: &Cli, id: &str) -> Result<InspectResult> {
    // `inspect` renders JSON without a --format flag, and returns an array
    // even for a single id.
    let raw = cli.run(&["inspect", id]).await?;
    let value: serde_json::Value = crate::cli::decode(&raw)?;
    Ok(match value {
        serde_json::Value::Array(mut items) if !items.is_empty() => items.remove(0),
        other => other,
    })
}

pub async fn start(cli: &Cli, id: &str) -> Result<()> {
    cli.ok(&["start", id]).await
}

pub async fn stop(cli: &Cli, id: &str) -> Result<()> {
    cli.ok(&["stop", id]).await
}

/// Apple has no `restart`, so it is a stop followed by a start.
///
/// A container that is already down must still come up, so a failing stop is
/// not fatal here — only the start is.
pub async fn restart(cli: &Cli, id: &str) -> Result<()> {
    let _ = cli.ok(&["stop", id]).await;
    cli.ok(&["start", id]).await
}

pub async fn kill(cli: &Cli, id: &str) -> Result<()> {
    cli.ok(&["kill", id]).await
}

pub async fn remove(cli: &Cli, id: &str, force: bool) -> Result<()> {
    let mut args = vec!["delete"];
    if force {
        args.push("--force");
    }
    args.push(id);
    cli.ok(&args).await
}

pub async fn prune(cli: &Cli) -> Result<PruneReport> {
    let before = list(cli, true).await.map(|c| c.len()).unwrap_or(0);
    cli.ok(&["prune"]).await?;
    let after = list(cli, true).await.map(|c| c.len()).unwrap_or(0);
    Ok(PruneReport {
        kind: "containers".into(),
        removed: before.saturating_sub(after) as i64,
        // Apple reports no per-prune byte count; `system df` is the honest
        // place to see space come back.
        reclaimed: 0,
    })
}

pub async fn logs(cli: &Cli, id: &str, tail: u32) -> Result<String> {
    let n = tail.to_string();
    cli.run(&["logs", "-n", &n, id]).await
}

/// Start a container from a run request.
///
/// Returns the container id, which for Apple's runtime is also its name.
pub async fn run(cli: &Cli, input: &RunInput) -> Result<String> {
    let argv = run_args(input);
    let borrowed: Vec<&str> = argv.iter().map(String::as_str).collect();
    let out = cli.run(&borrowed).await?;
    // `run --detach` prints the id; fall back to the requested name.
    let id = out.trim().lines().last().unwrap_or("").trim().to_string();
    if !id.is_empty() {
        return Ok(id);
    }
    input
        .name
        .clone()
        .ok_or_else(|| DockerError::decode("`container run` reported no id"))
}

/// Build the argument vector for `container run`.
///
/// Kept pure and separately tested: this is the translation layer where a
/// wrong flag means a container that silently comes up misconfigured.
pub fn run_args(input: &RunInput) -> Vec<String> {
    let mut a: Vec<String> = vec!["run".into(), "--detach".into()];

    if let Some(name) = &input.name {
        a.push("--name".into());
        a.push(name.clone());
    }
    for env in &input.env {
        a.push("--env".into());
        a.push(env.clone());
    }
    for p in &input.ports {
        a.push("--publish".into());
        a.push(match &p.proto {
            Some(proto) if !proto.is_empty() && proto != "tcp" => {
                format!("{}:{}/{}", p.host, p.container, proto)
            }
            _ => format!("{}:{}", p.host, p.container),
        });
    }
    for v in &input.volumes {
        a.push("--volume".into());
        a.push(if v.ro {
            format!("{}:{}:ro", v.host, v.container)
        } else {
            format!("{}:{}", v.host, v.container)
        });
    }
    if let Some(network) = &input.network {
        a.push("--network".into());
        a.push(network.clone());
    }
    if let Some(workdir) = &input.workdir {
        a.push("--workdir".into());
        a.push(workdir.clone());
    }
    if let Some(user) = &input.user {
        a.push("--user".into());
        a.push(user.clone());
    }
    if let Some(cpus) = input.limits.cpus {
        // Apple takes whole CPUs; round up so a 0.5 request still gets one
        // rather than being dropped.
        a.push("--cpus".into());
        a.push((cpus.ceil().max(1.0) as i64).to_string());
    }
    if let Some(bytes) = input.limits.memory {
        a.push("--memory".into());
        a.push(memory_arg(bytes));
    }
    for (k, v) in &input.labels {
        a.push("--label".into());
        a.push(format!("{k}={v}"));
    }
    if input.auto_remove {
        a.push("--rm".into());
    }
    if input.tty {
        a.push("--tty".into());
    }

    a.push(input.image.clone());

    if let Some(cmd) = &input.command {
        a.extend(split_command(cmd));
    }
    a
}

/// What a run request asks for that Apple's runtime cannot do.
///
/// Surfaced to the user rather than dropped silently — a container that comes
/// up without its restart policy should say so.
pub fn unsupported(input: &RunInput) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(restart) = &input.restart {
        if restart != "no" && !restart.is_empty() {
            out.push(format!(
                "Apple Containers has no restart policy, so `{restart}` was not applied."
            ));
        }
    }
    if input.hostname.is_some() {
        out.push("Apple Containers sets the hostname from the container name, so the hostname you gave was not applied.".into());
    }
    if input.limits.memory_reservation.is_some() {
        out.push("Apple Containers has no soft memory reservation, so that limit was not applied.".into());
    }
    if input.limits.pids_limit.is_some() {
        out.push("Apple Containers has no PID limit, so that limit was not applied.".into());
    }
    out
}

/// Bytes to the suffixed form Apple's `--memory` expects.
fn memory_arg(bytes: u64) -> String {
    const G: u64 = 1024 * 1024 * 1024;
    const M: u64 = 1024 * 1024;
    if bytes >= G && bytes.is_multiple_of(G) {
        format!("{}G", bytes / G)
    } else if bytes >= M {
        format!("{}M", bytes / M)
    } else {
        // Apple's granularity is 1MiB; anything smaller would round to zero.
        "1M".to_string()
    }
}

/// Split a command string into argv, honouring quotes.
///
/// The run dialog takes one free-text line, and `sh -c "echo hi"` has to reach
/// the daemon as three arguments rather than four.
pub fn split_command(cmd: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut any = false;

    for ch in cmd.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, c @ ('\'' | '"')) => {
                quote = Some(c);
                any = true;
            }
            (None, c) if c.is_whitespace() => {
                if any || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    any = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if any || !cur.is_empty() {
        out.push(cur);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{PortMapping, ResourceLimits, VolumeMapping};

    fn input() -> RunInput {
        RunInput {
            image: "nginx:latest".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_bare_run_detaches_and_ends_with_the_image() {
        let a = run_args(&input());
        assert_eq!(a[0], "run");
        assert!(a.contains(&"--detach".to_string()));
        assert_eq!(a.last().unwrap(), "nginx:latest");
    }

    #[test]
    fn ports_volumes_and_env_use_apples_spellings() {
        let mut i = input();
        i.name = Some("web".into());
        i.env = vec!["KEY=value".into()];
        i.ports = vec![PortMapping {
            host: "8080".into(),
            container: "80".into(),
            proto: None,
        }];
        i.volumes = vec![VolumeMapping {
            host: "data".into(),
            container: "/var/lib/data".into(),
            ro: true,
        }];
        let a = run_args(&i).join(" ");
        assert!(a.contains("--name web"));
        assert!(a.contains("--env KEY=value"));
        assert!(a.contains("--publish 8080:80"));
        assert!(a.contains("--volume data:/var/lib/data:ro"));
    }

    #[test]
    fn a_udp_port_carries_its_protocol_but_tcp_stays_bare() {
        let mut i = input();
        i.ports = vec![
            PortMapping { host: "53".into(), container: "53".into(), proto: Some("udp".into()) },
            PortMapping { host: "80".into(), container: "80".into(), proto: Some("tcp".into()) },
        ];
        let a = run_args(&i).join(" ");
        assert!(a.contains("--publish 53:53/udp"));
        assert!(a.contains("--publish 80:80"), "tcp is the default and reads cleaner bare");
        assert!(!a.contains("80:80/tcp"));
    }

    #[test]
    fn a_fractional_cpu_request_rounds_up_rather_than_disappearing() {
        // Apple allocates whole CPUs. Truncating 0.5 to 0 would ask for a
        // container with no processor at all.
        let mut i = input();
        i.limits = ResourceLimits { cpus: Some(0.5), ..Default::default() };
        let a = run_args(&i).join(" ");
        assert!(a.contains("--cpus 1"), "got {a}");
    }

    #[test]
    fn memory_is_rendered_with_the_suffix_apple_expects() {
        assert_eq!(memory_arg(2 * 1024 * 1024 * 1024), "2G");
        assert_eq!(memory_arg(512 * 1024 * 1024), "512M");
        assert_eq!(memory_arg(1536 * 1024 * 1024), "1536M");
        // Below Apple's 1MiB granularity, ask for the minimum rather than 0.
        assert_eq!(memory_arg(1024), "1M");
    }

    #[test]
    fn a_quoted_command_survives_as_one_argument() {
        assert_eq!(
            split_command(r#"sh -c "echo hello world""#),
            vec!["sh", "-c", "echo hello world"]
        );
        assert_eq!(split_command("  nginx  -g  daemon off;  "), vec!["nginx", "-g", "daemon", "off;"]);
    }

    #[test]
    fn an_empty_quoted_argument_is_kept() {
        // `--flag ""` means something; dropping it changes the command.
        assert_eq!(split_command(r#"app --tag "" x"#), vec!["app", "--tag", "", "x"]);
    }

    #[test]
    fn what_apple_cannot_do_is_reported_rather_than_dropped() {
        let mut i = input();
        i.restart = Some("always".into());
        i.hostname = Some("web.local".into());
        let notes = unsupported(&i);
        assert_eq!(notes.len(), 2);
        assert!(notes[0].contains("restart policy"));
        assert!(notes[1].contains("hostname"));
    }

    #[test]
    fn a_restart_policy_of_no_is_not_worth_warning_about() {
        let mut i = input();
        i.restart = Some("no".into());
        assert!(unsupported(&i).is_empty());
    }
}
