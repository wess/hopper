use model::EngineResources;
use serde_json::json;
use std::path::Path;

pub const INSTANCE: &str = "hopper";

pub fn render(resources: EngineResources, home: &Path) -> anyhow::Result<String> {
    let resources = resources.bounded();
    Ok(serde_yaml::to_string(&json!({
        "minimumLimaVersion": "2.2.1",
        "base": ["template:docker-rootful"],
        "vmType": "vz",
        "arch": "aarch64",
        "cpus": resources.cpus,
        "memory": format!("{}GiB", resources.memory_gib),
        "disk": format!("{}GiB", resources.disk_gib),
        "mountType": "virtiofs",
        "mounts": [{"location": home, "writable": true}],
        "ssh": {"loadDotSSHPubKeys": false, "forwardAgent": false},
        "portForwards": [
            {"guestIP": "0.0.0.0", "hostIP": "127.0.0.1", "guestPortRange": [1, 65535], "hostPortRange": [1, 65535]}
        ]
    }))?)
}
