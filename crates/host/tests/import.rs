//! Opt-in migration seam test: a source fixture and a real destination daemon.
//! Creates one uniquely named stopped container and removes it afterwards.

use docker::{containers, Client, Endpoint};
use host::Host;
use model::{MigrationEndpoint, MigrationPlan};
use serde_json::json;
use tokio::io::{AsyncReadExt, AsyncWriteExt};

#[tokio::test]
#[ignore = "requires HOPPER_TEST_HOST and a preloaded HOPPER_TEST_IMAGE on a live Docker daemon"]
async fn import_stays_stopped_and_a_second_import_cannot_claim_existing_data() -> anyhow::Result<()>
{
    let endpoint = std::env::var("HOPPER_TEST_HOST")?;
    let image = std::env::var("HOPPER_TEST_IMAGE")?;
    let endpoint = docker::endpoint::parse_docker_host(&endpoint, false)
        .ok_or_else(|| anyhow::anyhow!("invalid HOPPER_TEST_HOST"))?;
    let client = Client::new(endpoint);
    client.negotiate().await?;
    let name = format!("hopperimportcheck{}", std::process::id());
    anyhow::ensure!(
        containers::inspect(&client, &name).await.is_err(),
        "test name is already in use"
    );

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let source_name = name.clone();
    let source_image = image.clone();
    let source = tokio::spawn(async move {
        while let Ok((mut connection, _)) = listener.accept().await {
            let mut request = Vec::new();
            let mut chunk = [0; 4096];
            while !request.windows(4).any(|w| w == b"\r\n\r\n") && request.len() < 8192 {
                let n = connection.read(&mut chunk).await.unwrap();
                if n == 0 {
                    break;
                }
                request.extend_from_slice(&chunk[..n]);
            }
            let request = String::from_utf8_lossy(&request);
            let body = if request.contains("/version ") {
                json!({"ApiVersion":"1.44", "MinAPIVersion":"1.24"})
            } else if request.contains("/containers/json") {
                json!([{"Id":"fixture", "Names":[format!("/{source_name}")],"Image":source_image,"State":"running","Status":"Up","Mounts":[]}])
            } else {
                json!({"Config":{"Env":[],"Cmd":["sh","-c","echo should-not-run"],"Image":source_image},"HostConfig":{"RestartPolicy":{"Name":"always"}}})
            }.to_string();
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = connection.write_all(response.as_bytes()).await;
        }
    });
    let host = Host::new(client.clone());
    let plan = MigrationPlan {
        source: Some(MigrationEndpoint::from(Endpoint::Tcp {
            host: address.ip().to_string(),
            port: address.port(),
            tls: false,
        })),
        containers: vec!["fixture".into()],
        ..Default::default()
    };
    let result = async {
        let review = host.import_review(&plan).await;
        anyhow::ensure!(
            !review.blocked(),
            "preflight unexpectedly blocked: {review:?}"
        );
        let mut frames = Vec::new();
        let summary = host.import_run(&plan, &mut |p| frames.push(p)).await;
        anyhow::ensure!(
            !frames.iter().any(|p| p.error.is_some()),
            "import failed: {frames:?}"
        );
        anyhow::ensure!(summary.contains("Containers are stopped"));
        let created = containers::inspect(&client, &name).await?;
        anyhow::ensure!(created["State"]["Running"] == false);
        anyhow::ensure!(created["State"]["Status"] == "created");
        anyhow::ensure!(created["HostConfig"]["RestartPolicy"]["Name"] == "no");
        let review = host.import_review(&plan).await;
        anyhow::ensure!(review.blocked(), "existing container must block preflight");
        let mut frames = Vec::new();
        host.import_run(&plan, &mut |p| frames.push(p)).await;
        anyhow::ensure!(frames
            .last()
            .is_some_and(|p| p.finished && p.error.is_some()));
        let unchanged = containers::inspect(&client, &name).await?;
        anyhow::ensure!(
            unchanged["Id"] == created["Id"] && unchanged["State"]["Status"] == "created"
        );
        Ok(())
    }
    .await;
    source.abort();
    let cleanup = containers::remove(&client, &name, true, true).await;
    result?;
    cleanup?;
    Ok(())
}
