//! Safe installation of Hopper's bundled Docker command-line tools.

use model::{DockerCliSetupResult, DockerCliStatus};
use serde_json::Value;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;

fn sidecar(name: &str) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|error| error.to_string())?;
    let path = exe
        .parent()
        .ok_or_else(|| "Hopper's executable has no parent directory.".to_string())?
        .join("sidecars")
        .join(name);
    path.is_file()
        .then_some(path)
        .ok_or_else(|| format!("This Hopper build is missing its bundled {name} tool."))
}

fn owned_sidecar(path: &Path) -> bool {
    let Ok(target) = std::fs::read_link(path) else {
        return false;
    };
    if target
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return false;
    }
    let components: Vec<_> = target.components().map(|part| part.as_os_str()).collect();
    components.windows(4).any(|parts| {
        parts == ["Hopper.app", "Contents", "MacOS", "sidecars"].map(std::ffi::OsStr::new)
    })
}

fn removed_vendor_sidecar(target: &Path, home: Option<&Path>) -> bool {
    if target.exists() {
        return false;
    }
    let Ok(link) = std::fs::read_link(target) else {
        return false;
    };
    if link
        .components()
        .any(|part| matches!(part, std::path::Component::ParentDir))
    {
        return false;
    }
    let applications = [
        "Docker.app",
        "OrbStack.app",
        "Rancher Desktop.app",
        "Podman Desktop.app",
    ];
    applications.iter().any(|app| {
        link.starts_with(Path::new("/Applications").join(app).join("Contents"))
            || home.is_some_and(|home| {
                link.starts_with(home.join("Applications").join(app).join("Contents"))
            })
    })
}

#[cfg(unix)]
fn install_link(source: &Path, target: &Path) -> Result<(), String> {
    if target.is_symlink() {
        if std::fs::read_link(target).ok().as_deref() == Some(source) {
            return Ok(());
        }
        let ours = owned_sidecar(target);
        let home = std::env::var_os("HOME").map(PathBuf::from);
        if !ours && !removed_vendor_sidecar(target, home.as_deref()) {
            return Err(format!(
                "{} is already a symlink not owned by Hopper; it was left unchanged.",
                target.display()
            ));
        }
        let backup = target
            .parent()
            .ok_or("Missing CLI directory")?
            .join(format!(
                "hopperbackup{}",
                model::new_uuid().replace('-', "")
            ));
        std::fs::rename(target, &backup).map_err(|error| error.to_string())?;
        if let Err(error) = std::os::unix::fs::symlink(source, target) {
            let _ = std::fs::rename(&backup, target);
            return Err(error.to_string());
        }
        if ours {
            let _ = std::fs::remove_file(&backup);
        }
        // Preserve dangling links from removed vendors for manual recovery.
        return Ok(());
    } else if target.exists() {
        return Err(format!(
            "{} already exists and was left unchanged.",
            target.display()
        ));
    }
    std::os::unix::fs::symlink(source, target).map_err(|error| error.to_string())
}

fn run(docker: &Path, args: &[&str]) -> Result<(), String> {
    let output = Command::new(docker)
        .args(args)
        .env_remove("DOCKER_HOST")
        .env_remove("DOCKER_CONTEXT")
        .output()
        .map_err(|error| error.to_string())?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if detail.is_empty() {
        format!("docker {} failed with {}", args.join(" "), output.status)
    } else {
        detail
    })
}

fn configure_credential_store(path: &Path) -> Result<bool, String> {
    let mut config = if path.exists() {
        if std::fs::symlink_metadata(path)
            .map_err(|error| error.to_string())?
            .file_type()
            .is_symlink()
        {
            return Err(format!(
                "{} is a symlink and was left unchanged.",
                path.display()
            ));
        }
        serde_json::from_slice::<Value>(&std::fs::read(path).map_err(|error| error.to_string())?)
            .map_err(|error| format!("Cannot read {}: {error}", path.display()))?
    } else {
        Value::Object(Default::default())
    };
    let object = config
        .as_object_mut()
        .ok_or_else(|| format!("{} must contain a JSON object.", path.display()))?;
    match object.get("credsStore") {
        Some(Value::String(store)) if store == "desktop" => {}
        None => {}
        _ => return Ok(false),
    }
    object.insert("credsStore".into(), Value::String("osxkeychain".into()));

    let parent = path
        .parent()
        .ok_or_else(|| "Docker configuration has no parent directory.".to_string())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let mut bytes = serde_json::to_vec_pretty(&config).map_err(|error| error.to_string())?;
    bytes.push(b'\n');
    let temporary = (0..100)
        .find_map(|attempt| {
            let candidate = parent.join(format!(
                ".config.json.hopper-{}-{attempt}",
                std::process::id()
            ));
            let file = std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(&candidate)
                .ok()?;
            Some((candidate, file))
        })
        .ok_or_else(|| "Cannot create a private Docker configuration update.".to_string())?;
    let (temporary_path, mut temporary_file) = temporary;
    let publish = (|| -> Result<(), String> {
        temporary_file
            .write_all(&bytes)
            .and_then(|()| temporary_file.sync_all())
            .map_err(|error| error.to_string())?;
        std::fs::rename(&temporary_path, path).map_err(|error| error.to_string())?;
        std::fs::File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|error| error.to_string())
    })();
    if publish.is_err() {
        let _ = std::fs::remove_file(&temporary_path);
    }
    publish.map(|()| true)
}

pub fn configure(host: &str) -> DockerCliSetupResult {
    let result = (|| -> Result<DockerCliStatus, String> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is unavailable.".to_string())?;
        let docker = sidecar("docker")?;
        let compose = sidecar("compose")?;
        let buildx = sidecar("buildx")?;
        let credential_helper = sidecar("docker-credential-osxkeychain")?;
        let bin = home.join(".local/bin");
        let plugins = home.join(".docker/cli-plugins");
        std::fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&plugins).map_err(|error| error.to_string())?;
        install_link(&docker, &bin.join("docker"))?;
        install_link(&compose, &bin.join("docker-compose"))?;
        install_link(&compose, &plugins.join("docker-compose"))?;
        install_link(&buildx, &plugins.join("docker-buildx"))?;
        install_link(
            &credential_helper,
            &bin.join("docker-credential-osxkeychain"),
        )?;
        configure_credential_store(&home.join(".docker/config.json"))?;

        let exists = Command::new(&docker)
            .args(["context", "inspect", "hopper"])
            .env_remove("DOCKER_HOST")
            .env_remove("DOCKER_CONTEXT")
            .output()
            .map(|output| output.status.success())
            .unwrap_or(false);
        if exists {
            run(
                &docker,
                &[
                    "context",
                    "update",
                    "hopper",
                    "--docker",
                    &format!("host={host}"),
                ],
            )?;
        } else {
            run(
                &docker,
                &[
                    "context",
                    "create",
                    "hopper",
                    "--description",
                    "Hopper Engine",
                    "--docker",
                    &format!("host={host}"),
                ],
            )?;
        }
        run(&docker, &["context", "use", "hopper"])?;
        Ok(DockerCliStatus {
            available: true,
            configured: true,
            host: host.to_string(),
            context: Some("hopper".into()),
            detail: "Installed docker, Compose, Buildx, and macOS Keychain credentials; the hopper context is active."
                .into(),
            bundled: true,
        })
    })();

    match result {
        Ok(status) => DockerCliSetupResult {
            ok: true,
            detail: status.detail.clone(),
            status,
        },
        Err(detail) => DockerCliSetupResult {
            ok: false,
            detail,
            status: DockerCliStatus {
                host: host.to_string(),
                ..Default::default()
            },
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_hopper_app_sidecars_are_replaceable() {
        let dir = tempfile::tempdir().unwrap();
        let ours = dir.path().join("ours");
        let foreign = dir.path().join("foreign");
        std::os::unix::fs::symlink(
            "/Applications/Hopper.app/Contents/MacOS/sidecars/docker",
            &ours,
        )
        .unwrap();
        std::os::unix::fs::symlink("/opt/homebrew/bin/docker", &foreign).unwrap();
        assert!(owned_sidecar(&ours));
        assert!(!owned_sidecar(&foreign));
    }

    #[test]
    fn cli_handoff_preserves_unrelated_links_and_recognizes_only_removed_vendor_tools() {
        let directory = tempfile::tempdir().unwrap();
        let home = directory.path();
        let vendor = home.join("Applications/Docker.app/Contents/Resources/bin/docker");
        let dead = home.join("dead");
        std::os::unix::fs::symlink(&vendor, &dead).unwrap();
        assert!(removed_vendor_sidecar(&dead, Some(home)));
        std::fs::create_dir_all(vendor.parent().unwrap()).unwrap();
        std::fs::write(&vendor, "live").unwrap();
        assert!(
            !removed_vendor_sidecar(&dead, Some(home)),
            "a live competitor's CLI must remain intact"
        );
        let unrelated = home.join("unrelated");
        std::os::unix::fs::symlink(home.join("custom/docker"), &unrelated).unwrap();
        assert!(!removed_vendor_sidecar(&unrelated, Some(home)));
        assert!(install_link(&vendor, &unrelated).is_err());
        assert!(unrelated.is_symlink());
    }

    #[test]
    fn desktop_credentials_migrate_without_losing_registry_entries() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(
            &path,
            br#"{"auths":{"registry.example":{"auth":"secret"}},"credsStore":"desktop"}"#,
        )
        .unwrap();
        assert!(configure_credential_store(&path).unwrap());
        let config: Value = serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap();
        assert_eq!(config["credsStore"], "osxkeychain");
        assert_eq!(config["auths"]["registry.example"]["auth"], "secret");
    }

    #[test]
    fn explicit_non_desktop_credential_store_is_preserved() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        std::fs::write(&path, br#"{"credsStore":"custom"}"#).unwrap();
        assert!(!configure_credential_store(&path).unwrap());
        assert_eq!(
            std::fs::read_to_string(path).unwrap(),
            r#"{"credsStore":"custom"}"#
        );
    }
}
