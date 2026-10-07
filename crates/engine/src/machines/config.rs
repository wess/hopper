use anyhow::bail;
use model::{GuestOs, Machine, MachineProfile};
use serde_json::json;

pub fn profiles() -> Vec<MachineProfile> {
    [
        (
            "ubuntu",
            "Ubuntu Desktop",
            GuestOs::Linux,
            "Ubuntu 24.04 LTS ARM64. Downloads its desktop installer and prepares unattended setup. Installation verification and guest tools are in development.",
            Some("https://cdimage.ubuntu.com/ubuntu/releases/24.04/release/"),
            false,
            true,
        ),
        (
            "macos",
            "macOS",
            GuestOs::Macos,
            "Native macOS on Apple silicon. Restore-image discovery and runtime support are in development; installation and guest tools are not yet available.",
            None,
            false,
            true,
        ),
        (
            "windows",
            "Windows 11",
            GuestOs::Windows,
            "Windows 11 ARM64. Downloads and prepares an English (United States) installer on first start.",
            Some("https://www.microsoft.com/en-us/software-download/windows11arm64"),
            false,
            true,
        ),
    ]
    .into_iter()
    .map(
        |(id, name, guest, description, url, installer_required, experimental)| MachineProfile {
            id: id.into(),
            name: name.into(),
            guest,
            description: description.into(),
            download_url: url.map(str::to_string),
            installer_required,
            experimental,
        },
    )
    .collect()
}

pub fn render(machine: &Machine) -> anyhow::Result<String> {
    validate_resources(machine)?;
    let base = match machine.guest {
        GuestOs::Linux => "template:ubuntu-24.04",
        GuestOs::Macos => "template:macos-26",
        GuestOs::Windows => "template:windows-11",
    };
    let resources = machine.resources;
    let mut config = json!({
        "minimumLimaVersion": "2.2.1",
        "base": [base],
        "arch": "aarch64",
        "cpus": resources.cpus,
        "memory": format!("{}GiB", resources.memory_gib),
        "disk": format!("{}GiB", resources.disk_gib),
        "video": {"display": "default"},
        "mounts": [],
        "containerd": {"system": false, "user": false},
        "ssh": {"loadDotSSHPubKeys": false, "forwardAgent": false},
        "portForwards": [{"guestPortRange": [1,65535], "proto":"tcp", "ignore": true},{"guestPortRange": [1,65535], "proto":"udp", "ignore": true}]
    });
    if machine.guest != GuestOs::Windows {
        config["vmType"] = json!("vz");
    }
    if machine.guest == GuestOs::Linux {
        config["provision"] = json!([{"mode": "system", "script": r#"#!/bin/bash
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update
apt-get install -y --no-install-recommends xorg xfce4 xfce4-terminal lightdm dbus-x11 xdotool x11-utils scrot
mkdir -p /etc/lightdm/lightdm.conf.d
cat > /etc/lightdm/lightdm.conf.d/50-hopper.conf <<CONF
[Seat:*]
autologin-user={{.User}}
autologin-user-timeout=0
user-session=xfce
CONF
systemctl enable lightdm
systemctl restart lightdm
"#}]);
    }
    if machine.guest == GuestOs::Windows {
        if let Some(installer) = &machine.installer {
            if !std::path::Path::new(installer).is_absolute()
                || !std::path::Path::new(installer).is_file()
            {
                bail!("The Windows installer must be an existing absolute file path");
            }
            config["images"] = json!([{"location":installer,"arch":"aarch64"}]);
        }
    }
    Ok(serde_yaml::to_string(&config)?)
}

pub fn validate_resources(machine: &Machine) -> anyhow::Result<()> {
    let r = machine.resources;
    if !(1..=64).contains(&r.cpus)
        || !(1..=256).contains(&r.memory_gib)
        || !(10..=2048).contains(&r.disk_gib)
    {
        bail!("Use 1–64 CPUs, 1–256 GiB memory, and 10–2048 GiB disk");
    }
    if machine.guest != GuestOs::Linux && (r.cpus < 2 || r.memory_gib < 4 || r.disk_gib < 64) {
        bail!(
            "{} VMs need at least 2 CPUs, 4 GiB memory, and 64 GiB disk",
            machine.guest.label()
        );
    }
    Ok(())
}
