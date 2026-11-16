//! Build and validate Docker settings without embedding secrets in argv.

use model::DaemonConfig;
use serde_json::{json, Value};

pub fn document(config: &DaemonConfig) -> anyhow::Result<Value> {
    let mut value = json!({
        "log-driver": "local", "log-opts": {"max-size": "10m", "max-file": "3"},
        "features": {"cdi": true, "containerd-snapshotter": true},
        "builder": {"gc": {"enabled": true, "defaultKeepStorage": "5GB"}},
    });
    let map = value.as_object_mut().unwrap();
    for (key, values) in [
        ("insecure-registries", &config.insecure_registries),
        ("registry-mirrors", &config.registry_mirrors),
        ("dns", &config.dns),
    ] {
        if !values.is_empty() {
            map.insert(key.into(), json!(values));
        }
    }
    let mut proxies = serde_json::Map::new();
    for (key, proxy) in [
        ("http-proxy", &config.http_proxy),
        ("https-proxy", &config.https_proxy),
        ("no-proxy", &config.no_proxy),
    ] {
        if let Some(proxy) = proxy.as_ref().filter(|p| !p.trim().is_empty()) {
            proxies.insert(key.into(), json!(proxy));
        }
    }
    if !proxies.is_empty() {
        map.insert("proxies".into(), Value::Object(proxies));
    }
    if let Some(extra) = config.extra_json.as_ref().filter(|v| !v.trim().is_empty()) {
        anyhow::ensure!(
            extra.len() <= 1024 * 1024,
            "Daemon configuration exceeds 1 MiB."
        );
        let extra: Value = serde_json::from_str(extra)
            .map_err(|e| anyhow::anyhow!("Invalid Docker daemon JSON: {e}"))?;
        let extra = extra
            .as_object()
            .ok_or_else(|| anyhow::anyhow!("Docker daemon configuration must be a JSON object."))?;
        for key in [
            "hosts",
            "tls",
            "tlsverify",
            "tlscacert",
            "tlscert",
            "tlskey",
        ] {
            anyhow::ensure!(
                !extra.contains_key(key),
                "Hopper manages the Docker socket; {key} cannot be overridden."
            );
        }
        map.extend(extra.clone());
    }
    Ok(value)
}

pub fn apply_script(config: &DaemonConfig) -> anyhow::Result<String> {
    let document = serde_json::to_string(&document(config)?)?;
    // Compact JSON cannot contain a literal newline, so it cannot end this
    // quoted heredoc. The script travels on stdin and never enables tracing.
    Ok(format!(
        r#"set -eu
mkdir -p /etc/docker
candidate=$(mktemp /etc/docker/.hopper.XXXXXX)
backup=$(mktemp /etc/docker/.hopperbackup.XXXXXX)
had_config=0
trap 'rm -f "$candidate" "$backup"' EXIT
chmod 600 "$candidate" "$backup"
cat > "$candidate" <<'HOPPERCONFIG'
{document}
HOPPERCONFIG
dockerd --validate --config-file "$candidate" >/dev/null
if cmp -s "$candidate" /etc/docker/daemon.json; then exit 0; fi
if [ -f /etc/docker/daemon.json ]; then cp /etc/docker/daemon.json "$backup"; had_config=1; fi
mv "$candidate" /etc/docker/daemon.json
if systemctl restart docker && timeout 45 sh -c 'until docker info >/dev/null 2>&1; do sleep 1; done'; then
    echo 'Docker configuration applied and verified.'
else
    if [ "$had_config" = 1 ]; then mv "$backup" /etc/docker/daemon.json; else rm -f /etc/docker/daemon.json; fi
    systemctl restart docker || true
    echo 'Docker rejected the configuration; the previous configuration was restored.' >&2
    exit 1
fi
"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configuration_bounds_logs_and_cache_and_never_exposes_the_api_on_tcp() {
        let value = document(&DaemonConfig::default()).unwrap();
        assert_eq!(value["log-opts"]["max-size"], "10m");
        assert_eq!(value["builder"]["gc"]["defaultKeepStorage"], "5GB");
        for extra in [
            "[]",
            "{",
            r#"{"hosts":["tcp://0.0.0.0:2375"]}"#,
            r#"{"tlsverify":false}"#,
        ] {
            assert!(document(&DaemonConfig {
                extra_json: Some(extra.into()),
                ..Default::default()
            })
            .is_err());
        }
    }

    #[test]
    fn proxy_values_cannot_escape_the_stdin_heredoc() {
        let config = DaemonConfig {
            http_proxy: Some("x\nHOPPERCONFIG\ntouch /tmp/unwanted\n'$(id)".into()),
            ..Default::default()
        };
        let script = apply_script(&config).unwrap();
        assert_eq!(script.matches("\nHOPPERCONFIG\n").count(), 1);
        assert!(script.contains("\\nHOPPERCONFIG\\n"));
    }
}
