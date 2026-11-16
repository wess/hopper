//! Explicit replacement and removal review. Discovery and preview are read
//! only. Execution hands a fixed vendor/package-manager command to Terminal;
//! that Terminal asks for the application name again before executing it.

use crate::Host;
use model::{EngineInstallation, RemovalConsent, RemovalMethod, RemovalReview};
use std::path::Path;

pub fn command(installation: &EngineInstallation) -> Option<String> {
    match &installation.removal {
        RemovalMethod::DockerUninstaller { application } => Some(quote(
            &Path::new(application)
                .join("Contents/MacOS/uninstall")
                .to_string_lossy(),
        )),
        RemovalMethod::Homebrew {
            executable,
            package,
            cask,
        } => Some(format!(
            "env HOMEBREW_NO_AUTO_UPDATE=1 {} uninstall {} {}",
            quote(executable),
            if *cask { "--cask" } else { "--formula" },
            quote(package)
        )),
        RemovalMethod::Manual => None,
    }
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

pub fn consent(installation: &EngineInstallation, consent: &RemovalConsent) -> Result<(), String> {
    if !consent.backups_verified
        || !consent.workloads_verified
        || consent.application_name != installation.label
    {
        return Err("Verify independent backups and replacement workloads, then type the exact application name before continuing.".into());
    }
    Ok(())
}

fn consequences(id: &str) -> String {
    match id {
        "docker" => "Uninstalling Docker Desktop destroys its local containers, images, volumes and related data. Hopper's import does not copy volume contents. Back up databases and volumes independently and verify the replacement workloads first.",
        "podmandesktop" => "Removing Podman Desktop does not necessarily remove the Podman runtime or its machines. Stop unused machines separately to release resources. Data cleanup is a separate vendor action.",
        "colima" => "Removing the Colima package can leave its VM and container data behind. Stop Colima first. Hopper does not run colima delete or remove its data directory.",
        "orbstack" => "Removing OrbStack's app can leave its Linux machines and container data behind. Quit OrbStack first. Hopper does not delete its data directories.",
        _ => "Quit this tool and stop its workloads before removal. Package-manager hooks may remove application data; check the official instructions. Hopper does not purge shared configuration, VM disks, images or volumes itself.",
    }.into()
}

impl Host {
    pub async fn engine_installations(&self) -> Vec<EngineInstallation> {
        if cfg!(target_os = "macos") {
            return ::engine::installations::current();
        }
        self.engine_choices()
            .await
            .into_iter()
            .filter(|choice| {
                !choice.managed && choice.id != "existing" && (choice.installed || choice.connected)
            })
            .map(|choice| {
                let instructions = match choice.id.as_str() {
                    "podman" => "https://podman-desktop.io/docs/uninstall",
                    "colima" => "https://colima.run/docs/commands/",
                    "rancher" => "https://docs.rancherdesktop.io/getting-started/installation/",
                    "docker" if cfg!(target_os = "linux") => {
                        "https://docs.docker.com/engine/install/"
                    }
                    _ => "https://docs.docker.com/desktop/uninstall/",
                };
                EngineInstallation {
                    id: choice.id.clone(),
                    provider: choice.id,
                    label: choice.label,
                    location: choice.endpoint.unwrap_or_else(|| "Local runtime".into()),
                    application: None,
                    removal: RemovalMethod::Manual,
                    instructions: instructions.into(),
                }
            })
            .collect()
    }

    pub async fn removal_review(&self, id: &str) -> Result<RemovalReview, String> {
        let installation = self
            .engine_installations()
            .await
            .into_iter()
            .find(|item| item.id == id)
            .ok_or_else(|| {
                "That installation is no longer detected. Refresh the list.".to_string()
            })?;
        let status = self.engine_status().await;
        let mut blockers = Vec::new();
        if !cfg!(target_os = "macos") || status.provider != "vm" || !status.connected {
            blockers.push("Select Hopper Engine and start it before removing another runtime. Hopper must have a working replacement. Linux and Windows still require a Docker-compatible engine; use the official removal guide for redundant desktop applications.".into());
        }
        let preview = command(&installation);
        if preview.is_none() {
            blockers.push("This installation has no recognized automated uninstaller. Use the official removal instructions and your system's application manager; Refresh afterward to verify detection.".into());
        }
        Ok(RemovalReview {
            consequences: if cfg!(target_os = "macos") {
                consequences(id)
            } else {
                "Hopper needs an installed Docker-compatible engine on this platform. Keep one working; removing the only engine will prevent Hopper from running containers. Use the official system-specific instructions for a redundant desktop application.".into()
            },
            command: preview,
            installation,
            blockers,
        })
    }

    pub async fn launch_engine_removal(
        &self,
        id: &str,
        approval: &RemovalConsent,
    ) -> Result<String, String> {
        // Rebuild the plan from current filesystem evidence. The UI cannot
        // submit arbitrary executable paths or stale preview commands.
        let review = self.removal_review(id).await?;
        consent(&review.installation, approval)?;
        if !review.blockers.is_empty() {
            return Err(review.blockers.join(" "));
        }
        #[cfg(target_os = "macos")]
        {
            let (_, guard) = self
                .admit("removal_handoff")
                .await
                .map_err(|error| error.message)?;
            if id == "docker" {
                if let Some(application) = &review.installation.application {
                    verify_docker(application).await?;
                }
            }
            let current = self.engine_status().await;
            if current.provider != "vm" || !current.connected {
                return Err("Hopper Engine is no longer selected and connected. No removal process was launched.".into());
            }
            let directory = self
                .state_directory()
                .join("actions")
                .join(model::new_uuid().replace('-', ""));
            let mut builder = std::fs::DirBuilder::new();
            use std::os::unix::fs::{DirBuilderExt, PermissionsExt};
            builder
                .recursive(true)
                .mode(0o700)
                .create(&directory)
                .map_err(|e| e.to_string())?;
            let path = directory.join("remove.command");
            let docker::Endpoint::Unix { path: socket } = self.client().endpoint() else {
                return Err(
                    "Hopper Engine must use its private local socket before replacement.".into(),
                );
            };
            let script = removal_script(&review.installation, &path, Path::new(&socket))?;
            store::json::write_bytes(&path, script.as_bytes()).map_err(|e| e.to_string())?;
            std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700))
                .map_err(|e| e.to_string())?;
            let result = tokio::time::timeout(
                std::time::Duration::from_secs(10),
                tokio::process::Command::new("/usr/bin/open")
                    .args(["-a", "Terminal"])
                    .arg(&path)
                    .kill_on_drop(true)
                    .status(),
            )
            .await;
            if !matches!(result, Ok(Ok(status)) if status.success()) {
                let _ = std::fs::remove_file(path);
                let _ = std::fs::remove_dir(directory);
                return Err(
                    "Could not open the removal review in Terminal. No uninstaller was launched."
                        .into(),
                );
            }
            if let Some(guard) = guard {
                guard.finish(true);
            }
            Ok("Removal review opened in Terminal. Type the application name there to run the displayed command. Removal is not yet confirmed; return here and Refresh afterward. Configure Hopper's Docker CLI tools in Settings when finished.".into())
        }
        #[cfg(not(target_os = "macos"))]
        Err("Use your operating system's application manager and the official removal instructions.".into())
    }
}

#[cfg(target_os = "macos")]
async fn verify_docker(application: &str) -> Result<(), String> {
    let verify = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        tokio::process::Command::new("/usr/bin/codesign")
            .args(["--verify", "--strict", application])
            .kill_on_drop(true)
            .output(),
    )
    .await;
    if !matches!(verify, Ok(Ok(output)) if output.status.success()) {
        return Err("Docker Desktop's signature could not be verified. Use the official removal instructions instead.".into());
    }
    let identity = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        tokio::process::Command::new("/usr/bin/codesign")
            .args(["-dv", "--verbose=4", application])
            .kill_on_drop(true)
            .output(),
    )
    .await;
    match identity {
        Ok(Ok(output)) if output.status.success() && String::from_utf8_lossy(&output.stderr).lines().any(|line| line == "TeamIdentifier=9BNSXJN65R") => Ok(()),
        _ => Err("This Docker application does not have Docker's expected signing identity. Use its official removal instructions.".into()),
    }
}

#[cfg(any(test, target_os = "macos"))]
fn removal_script(
    installation: &EngineInstallation,
    path: &Path,
    socket: &Path,
) -> Result<String, String> {
    let command = command(installation)
        .ok_or_else(|| "No automated removal method is available.".to_string())?;
    let cleanup_file = quote(&path.to_string_lossy());
    let cleanup_directory = quote(
        &path
            .parent()
            .ok_or("Missing script directory")?
            .to_string_lossy(),
    );
    let title = quote(&format!("Remove {}", installation.label));
    let warning = quote(&consequences(&installation.id));
    let preview = quote(&format!("Command: {command}"));
    let prompt = quote(&format!(
        "Type {} to run the removal command: ",
        installation.label
    ));
    let label = quote(&installation.label);
    let socket = quote(&socket.to_string_lossy());
    Ok(format!(
        r#"#!/bin/sh
set -u
cleanup() {{ rm -f -- {cleanup_file}; rmdir -- {cleanup_directory} 2>/dev/null || :; }}
trap cleanup EXIT
printf '%s\n' {title} {warning} {preview}
printf '%s' {prompt}
IFS= read -r confirmation || exit 1
if [ "$confirmation" != {label} ]; then
  printf '%s\n' 'Cancelled. No uninstaller was run.'
  exit 1
fi
if [ "$(/usr/bin/curl --silent --show-error --fail --max-time 5 --unix-socket {socket} http://localhost/_ping)" != 'OK' ]; then
  printf '%s\n' 'Hopper Engine is unavailable. No uninstaller was run.'
  exit 1
fi
if {command}; then
  printf '%s\n' 'Removal command completed. Refresh Hopper to check the installation.'
else
  printf '%s\n' 'Removal command failed. Review the vendor output above; do not assume the application or its data was removed.'
fi
printf '%s' 'Press Return to close this review.'
IFS= read -r ignored || :
"#
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn installation() -> EngineInstallation {
        EngineInstallation {
            id: "docker".into(),
            provider: "docker".into(),
            label: "Docker Desktop".into(),
            location: "/Applications/Docker.app".into(),
            application: Some("/Applications/Docker.app".into()),
            removal: RemovalMethod::DockerUninstaller {
                application: "/Applications/Docker.app".into(),
            },
            instructions: "https://docs.docker.com/desktop/uninstall/".into(),
        }
    }
    #[test]
    fn migration_alone_is_never_removal_consent() {
        let item = installation();
        assert!(consent(&item, &RemovalConsent::default()).is_err());
        assert!(consent(
            &item,
            &RemovalConsent {
                application_name: item.label.clone(),
                backups_verified: true,
                workloads_verified: false
            }
        )
        .is_err());
        assert!(consent(
            &item,
            &RemovalConsent {
                application_name: item.label.clone(),
                backups_verified: true,
                workloads_verified: true
            }
        )
        .is_ok());
    }
    #[cfg(unix)]
    #[test]
    fn terminal_review_never_runs_before_exact_confirmation() {
        use std::io::Write;
        use std::os::unix::fs::PermissionsExt;
        use std::process::{Command, Stdio};
        let fixture = tempfile::tempdir().unwrap();
        let app = fixture.path().join("it's $(touch injected).app");
        std::fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        let marker = fixture.path().join("ran");
        let helper = app.join("Contents/MacOS/uninstall");
        std::fs::write(
            &helper,
            format!(
                "#!/bin/sh\nprintf 'ok' > {}\n",
                quote(&marker.to_string_lossy())
            ),
        )
        .unwrap();
        std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut item = installation();
        item.removal = RemovalMethod::DockerUninstaller {
            application: app.to_string_lossy().into(),
        };
        for (input, ready, expected) in [
            ("wrong\n", false, false),
            ("Docker Desktop\n\n", false, false),
            ("Docker Desktop\n\n", true, true),
        ] {
            let action = fixture.path().join("action");
            std::fs::create_dir(&action).unwrap();
            let path = action.join("remove.command");
            std::fs::write(
                &path,
                removal_script(&item, &path, &fixture.path().join("socket")).unwrap(),
            )
            .unwrap();
            let server = if ready {
                use std::io::Read;
                let listener =
                    std::os::unix::net::UnixListener::bind(fixture.path().join("socket")).unwrap();
                Some(std::thread::spawn(move || {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let mut request = [0; 4096];
                    let _ = stream.read(&mut request).unwrap();
                    stream
                        .write_all(
                            b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nOK",
                        )
                        .unwrap();
                }))
            } else {
                None
            };
            let mut child = Command::new("/bin/sh")
                .arg(&path)
                .current_dir(fixture.path())
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .spawn()
                .unwrap();
            child
                .stdin
                .take()
                .unwrap()
                .write_all(input.as_bytes())
                .unwrap();
            child.wait_with_output().unwrap();
            if let Some(server) = server {
                server.join().unwrap();
            }
            assert_eq!(marker.exists(), expected);
            assert!(!fixture.path().join("injected").exists());
            assert!(!action.exists());
        }
    }

    #[test]
    fn shell_paths_are_quoted_and_manual_installs_cannot_be_executed() {
        let mut item = installation();
        item.removal = RemovalMethod::DockerUninstaller {
            application: "/tmp/it's $(touch nope).app".into(),
        };
        let script = removal_script(
            &item,
            Path::new("/tmp/action/remove.command"),
            Path::new("/tmp/hopper.sock"),
        )
        .unwrap();
        assert!(script.contains("it'\\''s $(touch nope)"));
        assert!(script.contains("IFS= read -r confirmation"));
        assert!(!script.contains("rm -rf"));
        item.removal = RemovalMethod::Manual;
        assert!(command(&item).is_none());
        assert!(removal_script(
            &item,
            Path::new("/tmp/remove.command"),
            Path::new("/tmp/hopper.sock")
        )
        .is_err());
    }
}
