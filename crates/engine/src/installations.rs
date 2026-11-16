//! Installation evidence is independent of socket liveness. Filesystem-only
//! discovery runs on demand, never by launching every competing desktop app.

use model::{EngineInstallation, RemovalMethod};
use std::path::{Path, PathBuf};

struct Product {
    id: &'static str,
    provider: &'static str,
    label: &'static str,
    app: &'static str,
    binary: &'static str,
    package: &'static str,
    cask: bool,
    instructions: &'static str,
}

const PRODUCTS: &[Product] = &[
    Product {
        id: "docker",
        provider: "docker",
        label: "Docker Desktop",
        app: "Docker.app",
        binary: "",
        package: "docker-desktop",
        cask: true,
        instructions: "https://docs.docker.com/desktop/uninstall/",
    },
    Product {
        id: "podmandesktop",
        provider: "podman",
        label: "Podman Desktop",
        app: "Podman Desktop.app",
        binary: "",
        package: "podman-desktop",
        cask: true,
        instructions: "https://podman-desktop.io/docs/uninstall",
    },
    Product {
        id: "podman",
        provider: "podman",
        label: "Podman",
        app: "",
        binary: "podman",
        package: "podman",
        cask: false,
        instructions: "https://podman-desktop.io/docs/uninstall#uninstall-podman",
    },
    Product {
        id: "colima",
        provider: "colima",
        label: "Colima",
        app: "",
        binary: "colima",
        package: "colima",
        cask: false,
        instructions: "https://colima.run/docs/commands/",
    },
    Product {
        id: "rancher",
        provider: "rancher",
        label: "Rancher Desktop",
        app: "Rancher Desktop.app",
        binary: "",
        package: "rancher",
        cask: true,
        instructions: "https://docs.rancherdesktop.io/getting-started/installation/",
    },
    Product {
        id: "orbstack",
        provider: "orbstack",
        label: "OrbStack",
        app: "OrbStack.app",
        binary: "",
        package: "orbstack",
        cask: true,
        instructions: "https://docs.orbstack.dev/install#uninstalling",
    },
];

pub fn current() -> Vec<EngineInstallation> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    let paths = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
        .unwrap_or_default();
    detect(std::env::consts::OS, &home, &paths, &|path| path.exists())
}

pub fn detect(
    os: &str,
    home: &Path,
    paths: &[PathBuf],
    exists: &dyn Fn(&Path) -> bool,
) -> Vec<EngineInstallation> {
    if os != "macos" {
        return Vec::new();
    }
    PRODUCTS
        .iter()
        .filter_map(|product| {
            let application = (!product.app.is_empty())
                .then(|| {
                    [
                        PathBuf::from("/Applications").join(product.app),
                        home.join("Applications").join(product.app),
                    ]
                    .into_iter()
                    .find(|p| exists(p))
                })
                .flatten();
            let binary = (!product.binary.is_empty())
                .then(|| {
                    paths
                        .iter()
                        .chain(
                            [
                                Path::new("/opt/homebrew/bin").to_path_buf(),
                                Path::new("/usr/local/bin").to_path_buf(),
                                Path::new("/opt/podman/bin").to_path_buf(),
                            ]
                            .iter(),
                        )
                        .map(|p| p.join(product.binary))
                        .find(|p| exists(p))
                })
                .flatten();
            let packages: &[&str] = if product.id == "docker" {
                &["docker-desktop", "docker"]
            } else {
                &[product.package]
            };
            let package = ["/opt/homebrew", "/usr/local"]
                .into_iter()
                .find_map(|prefix| {
                    packages.iter().find_map(|name| {
                        let executable = Path::new(prefix).join("bin/brew");
                        let receipt = Path::new(prefix)
                            .join(if product.cask { "Caskroom" } else { "Cellar" })
                            .join(name);
                        (exists(&executable) && exists(&receipt))
                            .then_some((executable, receipt, *name))
                    })
                });
            let location = application
                .as_ref()
                .or(binary.as_ref())
                .or(package.as_ref().map(|(_, p, _)| p))?
                .to_string_lossy()
                .to_string();
            let removal = if let Some((executable, _, name)) = package {
                RemovalMethod::Homebrew {
                    executable: executable.to_string_lossy().into(),
                    package: name.into(),
                    cask: product.cask,
                }
            } else if product.id == "docker"
                && application
                    .as_ref()
                    .is_some_and(|app| exists(&app.join("Contents/MacOS/uninstall")))
            {
                RemovalMethod::DockerUninstaller {
                    application: application.as_ref().unwrap().to_string_lossy().into(),
                }
            } else {
                RemovalMethod::Manual
            };
            Some(EngineInstallation {
                id: product.id.into(),
                provider: product.provider.into(),
                label: product.label.into(),
                location,
                application: application.map(|p| p.to_string_lossy().into()),
                removal,
                instructions: product.instructions.into(),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stopped_app_is_detected_without_a_socket_and_cli_alone_is_not_docker_desktop() {
        let detected = detect(
            "macos",
            Path::new("/home/test"),
            &["/usr/local/bin".into()],
            &|p| {
                [
                    "/Applications/Docker.app",
                    "/Applications/Docker.app/Contents/MacOS/uninstall",
                    "/usr/local/bin/docker",
                ]
                .iter()
                .any(|entry| p == Path::new(entry))
            },
        );
        assert_eq!(detected.len(), 1);
        assert!(matches!(
            detected[0].removal,
            RemovalMethod::DockerUninstaller { .. }
        ));
        assert!(detect(
            "macos",
            Path::new("/home/test"),
            &["/usr/local/bin".into()],
            &|p| p == Path::new("/usr/local/bin/docker")
        )
        .is_empty());
    }
    #[test]
    fn homebrew_removal_requires_both_receipt_and_package_manager() {
        let all = [
            "/opt/homebrew/bin/colima",
            "/opt/homebrew/bin/brew",
            "/opt/homebrew/Cellar/colima",
        ];
        let scan = |entries: &[&str]| {
            detect("macos", Path::new("/test"), &[], &|p| {
                entries.iter().any(|entry| p == Path::new(entry))
            })
        };
        assert!(matches!(
            scan(&all)[0].removal,
            RemovalMethod::Homebrew { cask: false, .. }
        ));
        assert!(matches!(scan(&all[..1])[0].removal, RemovalMethod::Manual));
    }
    #[test]
    fn unsupported_platform_never_proposes_removing_a_required_native_daemon() {
        assert!(detect("linux", Path::new("/home/test"), &[], &|_| true).is_empty());
    }
}
