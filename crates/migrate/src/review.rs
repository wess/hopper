//! Read-only migration preflight. Never creates resources or starts workloads.

use docker::{Client, DockerError};
use model::{
    Container, MigrationPhase, MigrationPlan, MigrationProgress, MigrationReview, Network,
};
use serde_json::Value;

pub fn notice(
    phase: MigrationPhase,
    item: &str,
    message: impl Into<String>,
    error: bool,
) -> MigrationProgress {
    let message = message.into();
    MigrationProgress {
        phase,
        item: item.into(),
        done: 0,
        total: 0,
        message: if error { "Blocked" } else { "Review" }.into(),
        error: error.then(|| message.clone()),
        warning: (!error).then_some(message),
        finished: false,
    }
}

/// A name collision is a blocker, even if it looks similar. Migration must
/// never claim ownership of, overwrite, or count somebody else's data.
pub fn container_conflict(name: &str, destination: &[Container]) -> bool {
    destination.iter().any(|c| c.name == name)
}

/// Fields outside RunInput's portable contract. Report field names only:
/// inspect contains credentials and must never be included in diagnostics.
pub fn unsupported(raw: &Value) -> Vec<&'static str> {
    let mut fields = Vec::new();
    for field in [
        "CapAdd",
        "CapDrop",
        "SecurityOpt",
        "Devices",
        "DeviceRequests",
        "Ulimits",
        "Tmpfs",
        "ExtraHosts",
        "Dns",
        "DnsSearch",
        "Sysctls",
        "VolumesFrom",
        "Links",
        "GroupAdd",
        "CpusetCpus",
        "CpusetMems",
    ] {
        if raw
            .get("HostConfig")
            .and_then(|h| h.get(field))
            .is_some_and(populated)
        {
            fields.push(field);
        }
    }
    for field in ["CpuShares", "MemorySwap", "MemorySwappiness", "BlkioWeight"] {
        if raw["HostConfig"][field].is_number() && raw["HostConfig"][field].as_i64() != Some(0) {
            fields.push(field);
        }
    }
    for field in ["Privileged", "ReadonlyRootfs", "Init"] {
        if raw["HostConfig"][field].as_bool() == Some(true) {
            fields.push(field);
        }
    }
    for field in ["PidMode", "UsernsMode", "UTSMode", "CgroupnsMode"] {
        if raw["HostConfig"][field]
            .as_str()
            .is_some_and(|s| !s.is_empty() && s != "private")
        {
            fields.push(field);
        }
    }
    if raw["Config"]["Healthcheck"]
        .get("Test")
        .is_some_and(populated)
    {
        fields.push("Healthcheck");
    }
    if raw["NetworkSettings"]["Networks"]
        .as_object()
        .is_some_and(|n| n.len() > 1)
    {
        fields.push("Multiple networks");
    }
    if raw["HostConfig"]["PortBindings"]
        .as_object()
        .is_some_and(|ports| {
            ports.values().any(|bindings| {
                bindings.as_array().is_some_and(|bindings| {
                    bindings.iter().any(|b| {
                        b["HostIp"]
                            .as_str()
                            .is_some_and(|ip| !ip.is_empty() && ip != "0.0.0.0")
                    })
                })
            })
        })
    {
        fields.push("Port binding addresses");
    }
    fields
}

fn populated(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Array(v) => !v.is_empty(),
        Value::Object(v) => !v.is_empty(),
        Value::String(v) => !v.is_empty(),
        Value::Bool(v) => *v,
        Value::Number(v) => v.as_i64() != Some(0),
    }
}

pub async fn inspect(
    source: &Client,
    plan: &MigrationPlan,
    destination_containers: &[Container],
    destination_networks: &[Network],
) -> Result<MigrationReview, DockerError> {
    let mut review = MigrationReview::default();
    for volume in &plan.volumes {
        review.notices.push(notice(MigrationPhase::Volumes, volume,
            "Volume contents are not copied. Restore and verify the data before starting its containers.", false));
    }
    if !plan.containers.is_empty() {
        review.notices.push(notice(MigrationPhase::Containers, "",
            "Containers will be created stopped with automatic restart disabled. Writable container layers are not copied. Review data and settings before starting them, then restore restart policies as needed.", false));
        let containers = docker::containers::list(source, true).await?;
        for id in &plan.containers {
            let Some(container) = containers.iter().find(|c| c.id == *id || c.name == *id) else {
                review.notices.push(notice(
                    MigrationPhase::Containers,
                    id,
                    "The source container no longer exists.",
                    true,
                ));
                continue;
            };
            if container_conflict(&container.name, destination_containers) {
                review.notices.push(notice(MigrationPhase::Containers, &container.name,
                    "A container with this name already exists at the destination. Rename it or exclude this container.", true));
            }
            let raw = docker::containers::inspect(source, &container.id).await?;
            let unsupported = unsupported(&raw);
            if !unsupported.is_empty() {
                review.notices.push(notice(MigrationPhase::Containers, &container.name,
                    format!("These settings cannot be preserved by this import: {}. Reapply them before starting the container.", unsupported.join(", ")), false));
            }
            for mount in &container.mounts {
                let message = if mount.kind == "volume" {
                    Some(format!(
                        "Volume {} mounted at {} needs a separate data restore.",
                        mount.name.as_deref().unwrap_or(&mount.source),
                        mount.destination
                    ))
                } else if mount.kind == "bind" {
                    Some(format!("Verify host path {} is shared and contains the expected data at the destination.", mount.source))
                } else {
                    None
                };
                if let Some(message) = message {
                    review.notices.push(notice(
                        MigrationPhase::Containers,
                        &container.name,
                        message,
                        false,
                    ));
                }
            }
        }
    }
    if !plan.networks.is_empty() {
        let networks = docker::networks::list(source).await?;
        for id in &plan.networks {
            match networks.iter().find(|n| n.id == *id || n.name == *id) {
                None => review.notices.push(notice(
                    MigrationPhase::Networks,
                    id,
                    "The source network no longer exists.",
                    true,
                )),
                Some(network) => {
                    if destination_networks.iter().any(|n| n.name == network.name) {
                        review.notices.push(notice(MigrationPhase::Networks, &network.name,
                            "A network with this name already exists at the destination. Exclude it from the import and verify its configuration before use.", true));
                    }
                    if network.ipam.len() > 1 || !network.labels.is_empty() {
                        review.notices.push(notice(MigrationPhase::Networks, &network.name,
                            "Additional IPAM ranges and network labels are not preserved by this import.", false));
                    }
                }
            }
        }
    }
    Ok(review)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn review_reports_loss_without_leaking_inspect_values() {
        let raw = json!({"Config":{"Env":["PASSWORD=secret"], "Healthcheck":{"Test":["CMD", "true"]}}, "HostConfig":{"CapAdd":["SYS_ADMIN"],"ReadonlyRootfs":true,"Devices":[],"SecurityOpt":null,"PortBindings":{"80/tcp":[{"HostIp":"127.0.0.1"}]}},"NetworkSettings":{"Networks":{"a":{},"b":{}}}});
        assert_eq!(
            unsupported(&raw),
            vec![
                "CapAdd",
                "ReadonlyRootfs",
                "Healthcheck",
                "Multiple networks",
                "Port binding addresses"
            ]
        );
        assert!(!format!("{:?}", unsupported(&raw)).contains("secret"));
    }

    #[test]
    fn errors_block_and_warnings_do_not() {
        let mut review = MigrationReview {
            notices: vec![notice(MigrationPhase::Volumes, "db", "Restore data", false)],
        };
        assert!(!review.blocked());
        review
            .notices
            .push(notice(MigrationPhase::Containers, "db", "Conflict", true));
        assert!(review.blocked());
    }
}
