//! Opt-in comparison with Docker's own BuildKit context handling. Uses scratch
//! images, never starts a container, and cleans up only its unique resources.

use docker::{archive, build, containers, images, Client};
use model::{BuildInput, RunInput};
use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

async fn inventory(
    client: &Client,
    image: &str,
    archive_path: &Path,
) -> anyhow::Result<BTreeSet<String>> {
    let id = containers::create(
        client,
        &RunInput {
            image: image.into(),
            command: Some("/not-executed".into()),
            ..Default::default()
        },
    )
    .await?;
    let exported = archive::export_to(client, &id, "/data", archive_path).await;
    let cleanup = containers::remove(client, &id, true, false).await;
    exported?;
    cleanup?;
    let file = std::fs::File::open(archive_path)?;
    let mut tar = tar::Archive::new(file);
    let mut files = BTreeSet::new();
    for entry in tar.entries()? {
        let entry = entry?;
        let path = entry
            .path()?
            .to_string_lossy()
            .trim_end_matches('/')
            .to_string();
        if path.starts_with("data/")
            && ![
                "data/Dockerfile",
                "data/.dockerignore",
                "data/Dockerfile.dockerignore",
            ]
            .contains(&path.as_str())
        {
            files.insert(path);
        }
    }
    Ok(files)
}

#[tokio::test]
#[ignore = "requires HOPPER_TEST_HOST and Docker CLI with Buildx"]
async fn streamed_contexts_match_docker_buildkit_ignore_rules() -> anyhow::Result<()> {
    let host = std::env::var("HOPPER_TEST_HOST")?;
    let endpoint = docker::endpoint::parse_docker_host(&host, false)
        .ok_or_else(|| anyhow::anyhow!("Invalid HOPPER_TEST_HOST"))?;
    let client = Client::new(endpoint);
    client.negotiate().await?;
    for (index, (ignore, specific)) in [
        ("*.env\n", false),
        ("tree/file\n!tree\n", false),
        ("/root.txt/\n", false),
        ("**/*.env\n**/root.txt\n", false),
        ("{a,b}\n", false),
        ("[!a].txt\n", false),
        ("tree\n!tree/file\n", false),
        ("*.env\n", true),
    ]
    .into_iter()
    .enumerate()
    {
        let directory = tempfile::tempdir()?;
        std::fs::write(
            directory.path().join("Dockerfile"),
            "FROM scratch\nCOPY . /data/\n",
        )?;
        std::fs::create_dir(directory.path().join("empty"))?;
        for name in [
            "root.txt",
            "nested/root.txt",
            "secret.env",
            "nested/secret.env",
            "tree/file",
            "tree/other",
            "{a,b}",
            "a.txt",
            "!.txt",
            "b.txt",
            "foo/x.md",
            "deep/foo/file",
        ] {
            let path = directory.path().join(name);
            std::fs::create_dir_all(path.parent().unwrap())?;
            std::fs::write(path, b"fixture")?;
        }
        std::fs::write(
            directory.path().join(".dockerignore"),
            if specific { "*\n" } else { ignore },
        )?;
        if specific {
            std::fs::write(directory.path().join("Dockerfile.dockerignore"), ignore)?;
        }
        let ours = format!("hopperbuildcheck{}:stream{index}", std::process::id());
        let reference = format!("hopperbuildcheck{}:reference{index}", std::process::id());
        let result = async {
            let input = BuildInput {
                context_dir: directory.path().to_string_lossy().into(),
                tag: Some(ours.clone()),
                ..Default::default()
            };
            tokio::time::timeout(
                Duration::from_secs(90),
                build::run(&client, "build-check", &input, |_| {}),
            )
            .await??;
            let output = tokio::time::timeout(
                Duration::from_secs(90),
                tokio::process::Command::new(
                    std::env::var("HOPPER_TEST_DOCKER").unwrap_or_else(|_| "docker".into()),
                )
                .kill_on_drop(true)
                .env_remove("DOCKER_CONTEXT")
                .env("DOCKER_HOST", &host)
                .args(["buildx", "build", "--load", "--tag", &reference])
                .arg(directory.path())
                .output(),
            )
            .await??;
            anyhow::ensure!(
                output.status.success(),
                "BuildKit reference failed: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            let actual = inventory(&client, &ours, &directory.path().join("actual.tar")).await?;
            let expected =
                inventory(&client, &reference, &directory.path().join("expected.tar")).await?;
            anyhow::ensure!(
                actual == expected,
                "Ignore case {index} differs: Hopper={actual:?}, BuildKit={expected:?}"
            );
            anyhow::ensure!(
                actual.contains("data/empty"),
                "Empty directories must survive the streamed context"
            );
            Ok::<_, anyhow::Error>(())
        }
        .await;
        let _ = images::remove(&client, &ours, true).await;
        let _ = images::remove(&client, &reference, true).await;
        result?;
    }
    Ok(())
}
