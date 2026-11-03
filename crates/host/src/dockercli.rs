//! Safe installation of Hopper's bundled Docker command-line tools.

use model::{DockerCliSetupResult, DockerCliStatus};
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
    let text = target.to_string_lossy();
    text.contains("Hopper.app/Contents/MacOS/sidecars/")
}

#[cfg(unix)]
fn install_link(source: &Path, target: &Path) -> Result<(), String> {
    if target.is_symlink() {
        if std::fs::read_link(target).ok().as_deref() == Some(source) {
            return Ok(());
        }
        if !owned_sidecar(target) {
            return Err(format!(
                "{} is already a symlink not owned by Hopper; it was left unchanged.",
                target.display()
            ));
        }
        std::fs::remove_file(target).map_err(|error| error.to_string())?;
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

pub fn configure(host: &str) -> DockerCliSetupResult {
    let result = (|| -> Result<DockerCliStatus, String> {
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| "HOME is unavailable.".to_string())?;
        let docker = sidecar("docker")?;
        let compose = sidecar("compose")?;
        let buildx = sidecar("buildx")?;
        let bin = home.join(".local/bin");
        let plugins = home.join(".docker/cli-plugins");
        std::fs::create_dir_all(&bin).map_err(|error| error.to_string())?;
        std::fs::create_dir_all(&plugins).map_err(|error| error.to_string())?;
        install_link(&docker, &bin.join("docker"))?;
        install_link(&compose, &bin.join("docker-compose"))?;
        install_link(&compose, &plugins.join("docker-compose"))?;
        install_link(&buildx, &plugins.join("docker-buildx"))?;

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
                &["context", "update", "hopper", "--docker", &format!("host={host}")],
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
            detail: "Installed docker, Compose, and Buildx; the hopper context is active."
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
}
