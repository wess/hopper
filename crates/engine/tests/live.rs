//! Opt-in checks against Hopper's VM, including a graceful restart.

use docker::{archive, containers, exec, images, volumes, Client};
use engine::{providers::vm::Vm, Provider};
use model::{EngineResources, PortMapping, RunInput, VolumeMapping};
use std::collections::BTreeMap;
use std::time::Duration;

#[tokio::test]
#[ignore = "starts and restarts Hopper's VM; requires network access"]
async fn docker_files_ports_and_volumes_survive_a_vm_restart() -> anyhow::Result<()> {
    let vm = Vm::default();
    vm.start(EngineResources::default()).await?;
    let client = Client::new(vm.endpoint().await.unwrap());
    client.negotiate().await?;
    let pulled = images::pull(&client, "vm-check", "busybox:1.37", |_| {}).await?;
    anyhow::ensure!(pulled.ok, "image pull failed: {:?}", pulled.error);
    let name = format!("hoppercheck{}", std::process::id());
    let fixture = tempfile::tempdir_in(store::paths::root())?;
    std::fs::write(fixture.path().join("index.html"), "hopper vm ready")?;
    volumes::create(&client, &name, None, &BTreeMap::new()).await?;
    let listener = std::net::TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    let id = containers::create(
        &client,
        &RunInput {
            image: "busybox:1.37".into(),
            name: Some(name.clone()),
            command: Some("httpd -f -p 8080 -h /host".into()),
            restart: Some("unless-stopped".into()),
            ports: vec![PortMapping {
                host: port.to_string(),
                container: "8080".into(),
                proto: None,
            }],
            volumes: vec![
                VolumeMapping {
                    host: fixture.path().to_string_lossy().into(),
                    container: "/host".into(),
                    ro: false,
                },
                VolumeMapping {
                    host: name.clone(),
                    container: "/data".into(),
                    ro: false,
                },
            ],
            ..Default::default()
        },
    )
    .await?;
    let result = async {
        containers::start(&client, &id).await?;
        let (output, code) = exec::run_once(
            &client,
            &id,
            &[
                "sh".into(),
                "-c".into(),
                "cat /host/index.html; echo durable > /data/proof; echo shared > /host/fromguest"
                    .into(),
            ],
        )
        .await?;
        anyhow::ensure!(
            code == 0 && output.contains("hopper vm ready"),
            "exec or shared mount failed"
        );
        anyhow::ensure!(
            std::fs::read_to_string(fixture.path().join("fromguest"))?.trim() == "shared",
            "guest write did not reach host"
        );
        archive::write_file(&client, &id, "/data/archive", b"archive works").await?;
        anyhow::ensure!(
            archive::read_file(&client, &id, "/data/archive").await? == b"archive works",
            "archive round trip failed"
        );
        containers::pause(&client, &id).await?;
        containers::unpause(&client, &id).await?;
        wait_http(port).await?;
        vm.stop().await?;
        vm.start(EngineResources::default()).await?;
        client.negotiate().await?;
        wait_http(port).await?;
        let (output, code) =
            exec::run_once(&client, &id, &["cat".into(), "/data/proof".into()]).await?;
        anyhow::ensure!(
            code == 0 && output.trim() == "durable",
            "volume did not survive restart"
        );
        let docker::Endpoint::Unix { path } = client.endpoint() else {
            anyhow::bail!("the managed VM must expose a Unix socket");
        };
        std::fs::remove_file(path)?;
        anyhow::ensure!(vm.status().await.state == model::EngineState::Unreachable);
        vm.start(EngineResources::default()).await?;
        wait_http(port).await?;
        anyhow::Ok(())
    }
    .await;
    // cleanup only resources this check created, even when verification fails
    let removed = containers::remove(&client, &id, true, false).await;
    let volume = volumes::remove(&client, &name, false).await;
    result?;
    removed?;
    volume?;
    Ok(())
}

#[test]
#[ignore = "requires the bundled Lima helper and its templates"]
fn the_resolved_profile_forwards_the_docker_socket_once() -> anyhow::Result<()> {
    let fixture = tempfile::tempdir()?;
    let profile = fixture.path().join("hopper.yaml");
    let home = std::env::var_os("HOME").unwrap();
    std::fs::write(
        &profile,
        engine::vm::config::render(EngineResources::default(), std::path::Path::new(&home))?,
    )?;
    let output = std::process::Command::new(engine::vm::cli::locate().unwrap())
        .args(["template", "copy", "--embed-all", "--fill"])
        .arg(profile)
        .arg("-")
        .output()?;
    anyhow::ensure!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let config: serde_yaml::Value = serde_yaml::from_slice(&output.stdout)?;
    let sockets = config["portForwards"]
        .as_sequence()
        .unwrap()
        .iter()
        .filter(|entry| entry["guestSocket"].as_str() == Some("/var/run/docker.sock"))
        .count();
    anyhow::ensure!(
        sockets == 1,
        "expected one Docker socket forward, found {sockets}"
    );
    Ok(())
}

async fn wait_http(port: u16) -> anyhow::Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if let Ok(mut socket) = tokio::net::TcpStream::connect(("127.0.0.1", port)).await {
                socket
                    .write_all(b"GET / HTTP/1.0\r\nHost: localhost\r\n\r\n")
                    .await?;
                let mut response = String::new();
                socket.read_to_string(&mut response).await?;
                if response.contains("hopper vm ready") {
                    return anyhow::Ok(());
                }
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }
    })
    .await??;
    Ok(())
}
