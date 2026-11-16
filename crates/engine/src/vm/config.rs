use model::{EngineResources, Settings, SharedPath};
use serde_json::json;
use std::path::Path;

pub const INSTANCE: &str = "hopper";

pub fn render(resources: EngineResources, _home: &Path) -> anyhow::Result<String> {
    render_configured(&Settings {
        resources,
        ..Default::default()
    })
}

pub fn render_configured(settings: &Settings) -> anyhow::Result<String> {
    let resources = settings.resources.bounded();
    Ok(serde_yaml::to_string(&json!({
        "minimumLimaVersion": "2.2.1",
        "images": [{
            "location": "https://cloud-images.ubuntu.com/releases/resolute/release-20260927/ubuntu-26.04-server-cloudimg-arm64.img",
            "arch": "aarch64",
            "digest": "sha256:63a93bd5a8d76e33b15ceb5daa3657bd79be804748051ab178e643b0f5da22e7"
        }],
        "param": {"internal_netplanOptional": "true"},
        "containerd": {"system": false, "user": false},
        "provision": [{"mode": "system", "script": include_str!("bootstrap.sh")}],
        "probes": [{"script": "#!/bin/sh\nset -eu\ntimeout 60 sh -c 'until docker info >/dev/null 2>&1; do sleep 1; done'", "hint": "See /var/log/cloud-init-output.log inside Hopper's VM."}],
        "hostResolver": {"hosts": {"host.docker.internal": "host.lima.internal"}},
        "vmType": "vz",
        "arch": "aarch64",
        "cpus": resources.cpus,
        "memory": format!("{}GiB", resources.memory_gib),
        "disk": format!("{}GiB", resources.disk_gib),
        "mountType": "virtiofs",
        "mounts": mounts(settings)?,
        "ssh": {"loadDotSSHPubKeys": false, "forwardAgent": false},
        "portForwards": [
            {"guestSocket": "/var/run/docker.sock", "hostSocket": "{{.Dir}}/sock/docker.sock"},
            {"guestIP": "0.0.0.0", "hostIP": "127.0.0.1", "guestPortRange": [1, 65535], "hostPortRange": [1, 65535]}
        ]
    }))?)
}

pub fn shares(settings: &Settings) -> anyhow::Result<Vec<SharedPath>> {
    let mut shares = Vec::<SharedPath>::new();
    for share in settings
        .shares
        .iter()
        .cloned()
        .chain(settings.shared_paths.iter().map(|path| SharedPath {
            path: path.clone(),
            writable: false,
        }))
    {
        let path = Path::new(&share.path);
        anyhow::ensure!(
            path.is_absolute(),
            "Shared directories must use absolute paths."
        );
        let path = path
            .canonicalize()
            .map_err(|_| anyhow::anyhow!("Shared directory does not exist: {}", share.path))?;
        anyhow::ensure!(
            path.is_dir() && path.parent().is_some(),
            "Choose a directory other than the filesystem root."
        );
        let path_string = path
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("Shared paths must be valid UTF-8."))?;
        anyhow::ensure!(
            !path_string.contains("{{") && !path_string.contains("}}"),
            "Shared paths cannot contain Lima template expressions."
        );
        for existing in &shares {
            let other = Path::new(&existing.path);
            anyhow::ensure!(
                !path.starts_with(other) && !other.starts_with(&path),
                "Shared directories must not overlap: {} and {}",
                existing.path,
                path.display()
            );
        }
        shares.push(SharedPath {
            path: path_string.into(),
            writable: share.writable,
        });
    }
    Ok(shares)
}

fn mounts(settings: &Settings) -> anyhow::Result<serde_json::Value> {
    Ok(serde_json::Value::Array(
        shares(settings)?
            .iter()
            .map(|share| {
                json!({
                    "location": share.path, "mountPoint": share.path, "writable": share.writable,
                })
            })
            .collect(),
    ))
}

/// Applied after Lima resolves the base template, including its default home
/// mount. JSON encoding keeps paths out of yq's expression syntax.
pub fn mount_expression(settings: &Settings) -> anyhow::Result<String> {
    let mounts = mounts(settings)?;
    Ok(format!(
        ".mounts = {}",
        if mounts.as_array().is_some_and(|m| m.is_empty()) {
            "null".into()
        } else {
            serde_json::to_string(&mounts)?
        }
    ))
}

/// Replace legacy remote installers on the next stopped-VM start. The script
/// retains an installed service and uses the pinned archive for first boot or
/// recovery of an interrupted first provisioning only.
pub fn provision_expression() -> anyhow::Result<String> {
    Ok(format!(
        ".provision = {}",
        serde_json::to_string(&json!([
            {"mode": "system", "script": include_str!("bootstrap.sh")}
        ]))?
    ))
}
