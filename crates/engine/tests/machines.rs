use engine::machines::{config, Actor, Machines};
use model::{CreateMachine, EngineResources, GuestOs, Machine, MachineInput};

fn machine() -> Machine {
    Machine {
        id: "8197e0f0-0603-43e9-a817-eaf7ab0327af".into(),
        name: "Development".into(),
        guest: GuestOs::Linux,
        profile: "ubuntu".into(),
        resources: EngineResources::default(),
        agent_access: false,
        agent_generation: 0,
        runtime: None,
        installer: None,
    }
}

fn fixture() -> (tempfile::TempDir, Machines, Machine) {
    let root = tempfile::tempdir().unwrap();
    let manager = Machines {
        root: root.path().into(),
    };
    let machine = machine();
    store::json::write(
        &root
            .path()
            .join("records")
            .join(format!("{}.json", machine.id)),
        &machine,
    )
    .unwrap();
    (root, manager, machine)
}

#[test]
fn new_requests_enable_agent_access_by_default_but_old_records_do_not() {
    let request:CreateMachine=serde_json::from_value(serde_json::json!({"name":"Test","profile":"ubuntu","resources":{"cpus":2,"memoryGib":4,"diskGib":64}})).unwrap();
    assert!(request.agent_access);
    let mut value = serde_json::to_value(machine()).unwrap();
    value.as_object_mut().unwrap().remove("agentAccess");
    let old: Machine = serde_json::from_value(value).unwrap();
    assert!(!old.agent_access);
    assert_eq!(old.agent_generation, 0);
}

#[test]
fn profiles_do_not_share_the_host_or_forward_services() {
    let value: serde_yaml::Value =
        serde_yaml::from_str(&config::render(&machine()).unwrap()).unwrap();
    assert!(value["mounts"].as_sequence().unwrap().is_empty());
    assert_eq!(value["ssh"]["forwardAgent"].as_bool(), Some(false));
    assert_eq!(value["containerd"]["user"].as_bool(), Some(false));
    for rule in value["portForwards"].as_sequence().unwrap() {
        assert_eq!(rule["ignore"].as_bool(), Some(true));
    }
    assert_eq!(value["portForwards"][0]["proto"].as_str(), Some("tcp"));
    assert_eq!(value["portForwards"][1]["proto"].as_str(), Some("udp"));
    assert!(value["provision"][0]["script"]
        .as_str()
        .unwrap()
        .contains("scrot"));
}

#[test]
fn invalid_resources_are_rejected_instead_of_silently_replaced() {
    let mut machine = machine();
    machine.resources.cpus = 0;
    assert!(config::render(&machine).is_err());
    machine.resources = EngineResources::default();
    machine.guest = GuestOs::Macos;
    assert!(config::render(&machine)
        .unwrap_err()
        .to_string()
        .contains("64 GiB"));
    machine.resources.disk_gib = 64;
    assert!(config::render(&machine).is_ok());
}

#[test]
fn permission_changes_are_read_fresh_across_manager_instances() {
    let (_root, manager, machine) = fixture();
    let second = manager.clone();
    assert!(second.machine(&machine.id, Actor::Agent).is_err());
    manager.set_agent_access(&machine.id, true).unwrap();
    assert!(second.machine(&machine.id, Actor::Agent).is_ok());
    manager.set_agent_access(&machine.id, false).unwrap();
    assert!(second.machine(&machine.id, Actor::Agent).is_err());
}

#[tokio::test]
async fn disabled_agent_access_blocks_every_existing_vm_operation_before_launch() {
    let (root, manager, machine) = fixture();
    let id = &machine.id;
    let results = [
        manager.start(id, Actor::Agent).await,
        manager.stop(id, Actor::Agent).await,
        manager
            .exec(id, Actor::Agent, &["uname".into()])
            .await
            .map(|_| ()),
        manager
            .input(
                id,
                Actor::Agent,
                MachineInput::Key {
                    keys: "Return".into(),
                },
            )
            .await,
        manager
            .screenshot(id, Actor::Agent, &root.path().join("screen.png"))
            .await,
        manager
            .copy(
                id,
                Actor::Agent,
                &root.path().join("file"),
                "/tmp/file",
                true,
            )
            .await,
        manager
            .clone_machine(id, Actor::Agent, "Copy")
            .await
            .map(|_| ()),
        manager
            .snapshot(id, Actor::Agent, "Before change")
            .await
            .map(|_| ()),
        manager
            .restore(id, Actor::Agent, "8197e0f0-0603-43e9-a817-eaf7ab0327af")
            .await,
    ];
    for result in results {
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Agent access is off"));
    }
    assert!(!root.path().join("lima").exists());
    assert!(!root.path().join("screen.png").exists());
}

#[test]
fn ids_cannot_escape_the_machine_directory() {
    let (_root, manager, _) = fixture();
    for id in [
        "../settings",
        "/tmp/anything",
        "8197e0f0-0603-43e9-a817-eaf7ab0327a/",
        "",
    ] {
        assert!(manager
            .machine(id, Actor::Person)
            .unwrap_err()
            .to_string()
            .contains("Invalid VM id"));
        assert!(manager.set_agent_access(id, true).is_err());
    }
}

#[test]
fn windows_has_no_previous_runtime_template() {
    let mut machine = machine();
    machine.guest = GuestOs::Windows;
    machine.resources.disk_gib = 64;
    assert!(config::render(&machine).unwrap_err().to_string().contains("native runtime"));
    assert!(!config::profiles().iter().find(|profile| profile.id == "windows").unwrap().installer_required);
    let file = tempfile::NamedTempFile::new().unwrap();
    machine.installer = Some(file.path().to_string_lossy().into_owned());
    assert!(config::render(&machine).is_err());
    assert!(file.path().exists());
}

#[test]
fn access_can_be_revoked_while_a_vm_operation_holds_its_lock() {
    use fs2::FileExt;
    let (_root, manager, machine) = fixture();
    manager.set_agent_access(&machine.id, true).unwrap();
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .read(true)
        .write(true)
        .open(manager.root.join("locks").join(&machine.id))
        .unwrap();
    lock.lock_exclusive().unwrap();
    manager.set_agent_access(&machine.id, false).unwrap();
    assert!(manager.machine(&machine.id, Actor::Agent).is_err());
}

#[test]
fn exhausted_policy_generations_still_allow_revocation_and_prevent_reenable() {
    let root = tempfile::tempdir().unwrap();
    let manager = engine::machines::Machines { root: root.path().into() };
    let mut record = machine();
    record.agent_access = true;
    record.agent_generation = u64::MAX;
    store::json::write(&root.path().join("records").join(format!("{}.json", record.id)), &record).unwrap();
    manager.set_agent_access(&record.id, false).unwrap();
    assert!(manager.machine(&record.id, engine::machines::Actor::Agent).is_err());
    assert!(manager.set_agent_access(&record.id, true).is_err());
    assert!(!manager.machine(&record.id, engine::machines::Actor::Person).unwrap().agent_access);
}
