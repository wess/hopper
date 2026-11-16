use engine::vm::{cli::parse_instances, config};
use model::EngineResources;
use std::path::Path;

#[test]
fn shares_are_explicit_and_default_to_read_only() {
    let directory = tempfile::tempdir().unwrap();
    let settings = model::Settings {
        shared_paths: vec![directory.path().to_string_lossy().into()],
        ..Default::default()
    };
    let shares = config::shares(&settings).unwrap();
    assert_eq!(shares.len(), 1);
    assert!(!shares[0].writable);
    let mount = config::mount_expression(&settings).unwrap();
    assert!(mount.contains("\"writable\":false"));
    assert_eq!(
        config::mount_expression(&model::Settings::default()).unwrap(),
        ".mounts = null"
    );
    let mut overlapping = settings;
    overlapping.shares.push(model::SharedPath {
        path: directory.path().to_string_lossy().into(),
        writable: true,
    });
    assert!(config::shares(&overlapping).is_err());
    overlapping.shared_paths.clear();
    let child = directory.path().join("child");
    std::fs::create_dir(&child).unwrap();
    overlapping.shares.push(model::SharedPath {
        path: child.to_string_lossy().into(),
        writable: false,
    });
    assert!(config::shares(&overlapping).is_err());
}

#[tokio::test]
async fn lifecycle_locks_exclude_other_controllers_and_release_on_drop() {
    let directory = tempfile::tempdir().unwrap();
    let first = engine::vm::cli::lifecycle(directory.path()).await.unwrap();
    assert!(engine::vm::cli::lifecycle(directory.path()).await.is_err());
    drop(first);
    engine::vm::cli::lifecycle(directory.path())
        .await
        .expect("lock must release after drop");
}

#[cfg(unix)]
#[tokio::test]
async fn helper_input_is_closed_and_secret_output_is_never_reflected_on_failure() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().unwrap();
    let bin = directory.path().join("helper");
    std::fs::write(&bin, "#!/bin/sh\ncat >&2\nexit 3\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cli = engine::vm::cli::Cli {
        bin,
        home: directory.path().join("home"),
    };
    let error = cli
        .input(
            &[],
            b"private-proxy-password",
            std::time::Duration::from_secs(2),
        )
        .await
        .unwrap_err();
    assert!(!error.to_string().contains("private-proxy-password"));
    assert!(!cli.home.join("engine.log").exists());
    assert_eq!(
        std::fs::metadata(cli.home).unwrap().permissions().mode() & 0o777,
        0o700
    );
}

#[test]
fn profile_keeps_the_vm_private_and_exposes_rootful_docker() {
    let raw = config::render(EngineResources::default(), Path::new("/Users/some one")).unwrap();
    let value: serde_yaml::Value = serde_yaml::from_str(&raw).unwrap();
    assert_eq!(value["vmType"].as_str(), Some("vz"));
    assert!(value["base"].is_null());
    assert!(value["images"][0]["digest"]
        .as_str()
        .unwrap()
        .starts_with("sha256:"));
    let script = value["provision"][0]["script"].as_str().unwrap();
    assert!(script.contains("sha256sum --check"));
    assert!(!script.contains("get.docker.com"));
    assert!(value["mounts"].as_sequence().unwrap().is_empty());
    assert_eq!(
        value["portForwards"][0]["guestSocket"].as_str(),
        Some("/var/run/docker.sock")
    );
    assert_eq!(
        value["portForwards"][1]["hostIP"].as_str(),
        Some("127.0.0.1")
    );
    assert_eq!(value["ssh"]["forwardAgent"].as_bool(), Some(false));
}

#[test]
fn corrupted_resource_settings_are_bounded() {
    let raw = config::render(
        EngineResources {
            cpus: 0,
            memory_gib: 0,
            disk_gib: 0,
        },
        Path::new("/Users/test"),
    )
    .unwrap();
    let value: serde_yaml::Value = serde_yaml::from_str(&raw).unwrap();
    assert_eq!(value["cpus"].as_u64(), Some(1));
    assert_eq!(value["memory"].as_str(), Some("1GiB"));
    assert_eq!(value["disk"].as_str(), Some("10GiB"));
}

#[test]
fn status_only_reads_hoppers_instance() {
    assert!(parse_instances("").unwrap().is_none());
    let raw = "{\"name\":\"other\",\"status\":\"Running\"}\n{\"name\":\"hopper\",\"status\":\"Stopped\"}\n";
    assert_eq!(parse_instances(raw).unwrap().unwrap().status, "Stopped");
    assert!(parse_instances("not json").is_err());
}

#[test]
fn both_bundle_entry_points_find_the_vm_helper() {
    use engine::vm::cli::bundle_paths;
    let helper = Path::new("/Applications/Hopper.app/Contents/Resources/lima/bin/limactl");
    for exe in [
        "/Applications/Hopper.app/Contents/MacOS/hopper",
        "/Applications/Hopper.app/Contents/MacOS/sidecars/hoppermcp",
    ] {
        assert!(bundle_paths(Path::new(exe)).contains(&helper.to_path_buf()));
    }
}

#[cfg(unix)]
#[tokio::test]
async fn helper_failures_keep_logs_and_use_a_private_lima_home() {
    use engine::vm::cli::Cli;
    use std::os::unix::fs::PermissionsExt;
    let fixture = tempfile::tempdir().unwrap();
    let bin = fixture.path().join("helper");
    std::fs::write(&bin, "#!/bin/sh\nprintf '%s' \"$LIMA_HOME\" > \"$LIMA_HOME/context\"\necho 'test failure' >&2\nexit 7\n").unwrap();
    std::fs::set_permissions(&bin, std::fs::Permissions::from_mode(0o700)).unwrap();
    let cli = Cli {
        bin,
        home: fixture.path().join("private"),
    };
    let result = cli
        .run(&["start".into()], std::time::Duration::from_secs(2))
        .await;
    assert!(result.unwrap_err().to_string().contains("engine.log"));
    assert_eq!(
        std::fs::read_to_string(cli.home.join("context")).unwrap(),
        cli.home.to_string_lossy()
    );
    assert!(std::fs::read_to_string(cli.home.join("engine.log"))
        .unwrap()
        .contains("test failure"));
}

#[test]
fn legacy_provisioning_is_replaced_without_forcing_runtime_upgrades() {
    let expression = config::provision_expression().unwrap();
    let list: serde_json::Value =
        serde_json::from_str(expression.strip_prefix(".provision = ").unwrap()).unwrap();
    assert_eq!(list.as_array().unwrap().len(), 1);
    let script = list[0]["script"].as_str().unwrap();
    assert!(script.contains("systemctl cat docker.service"));
    assert!(script.contains("sha256sum --check"));
    assert!(!script.contains("get.docker.com"));
}
