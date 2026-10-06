use engine::vm::{cli::parse_instances, config};
use model::EngineResources;
use std::path::Path;

#[test]
fn profile_keeps_the_vm_private_and_exposes_rootful_docker() {
    let raw = config::render(EngineResources::default(), Path::new("/Users/some one")).unwrap();
    let value: serde_yaml::Value = serde_yaml::from_str(&raw).unwrap();
    assert_eq!(value["vmType"].as_str(), Some("vz"));
    assert_eq!(value["base"][0].as_str(), Some("template:docker-rootful"));
    assert_eq!(
        value["mounts"][0]["location"].as_str(),
        Some("/Users/some one")
    );
    // the rootful base owns socket forwarding; duplicating it unlinks the socket
    assert!(value["portForwards"][0]["guestSocket"].is_null());
    assert_eq!(
        value["portForwards"][0]["hostIP"].as_str(),
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
