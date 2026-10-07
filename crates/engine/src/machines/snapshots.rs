use super::{validate_id, Actor, Machines};
use anyhow::{bail, Context};
use model::MachineSnapshot;
use std::path::Path;
use std::time::Duration;

async fn copy_tree(source: &Path, target: &Path) -> anyhow::Result<()> {
    if !cfg!(target_os = "macos") {
        bail!("VM snapshots currently require macOS and an APFS volume");
    }
    let status = tokio::time::timeout(
        Duration::from_secs(300),
        tokio::process::Command::new("/bin/cp")
            .args(["-cR", "--"])
            .arg(source)
            .arg(target)
            .kill_on_drop(true)
            .status(),
    )
    .await
    .context("Snapshot copy timed out")??;
    if !status.success() {
        bail!("Could not copy the VM snapshot. The machine directory must be on APFS.");
    }
    Ok(())
}

impl Machines {
    pub fn snapshots(&self, id: &str, actor: Actor) -> anyhow::Result<Vec<MachineSnapshot>> {
        self.machine(id, actor)?;
        let dir = self.root.join("snapshots").join(id);
        if !dir.exists() {
            return Ok(vec![]);
        }
        let mut snapshots = vec![];
        for file in std::fs::read_dir(dir)? {
            let path = file?.path().join("snapshot.json");
            if path.is_file() {
                let snapshot: MachineSnapshot = serde_json::from_slice(&std::fs::read(path)?)?;
                validate_id(&snapshot.id)?;
                if snapshot.machine_id != id {
                    bail!("Snapshot belongs to a different VM");
                }
                snapshots.push(snapshot);
            }
        }
        snapshots.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(snapshots)
    }

    pub async fn snapshot(
        &self,
        id: &str,
        actor: Actor,
        name: &str,
    ) -> anyhow::Result<MachineSnapshot> {
        let _lock = self.lock(id)?;
        self.machine(id, actor)?;
        self.require_stopped(id).await?;
        if name.trim().is_empty() || name.len() > 100 {
            bail!("Choose a snapshot name of 1–100 characters");
        }
        let snapshot = MachineSnapshot {
            id: model::new_uuid(),
            name: name.trim().into(),
            machine_id: id.into(),
        };
        let dir = self.root.join("snapshots").join(id).join(&snapshot.id);
        std::fs::create_dir_all(&dir)?;
        let source = self.root.join("lima").join(id);
        copy_tree(&source, &dir.join("instance")).await?;
        store::json::write(&dir.join("snapshot.json"), &snapshot)?;
        Ok(snapshot)
    }

    pub async fn restore(&self, id: &str, actor: Actor, snapshot_id: &str) -> anyhow::Result<()> {
        let _lock = self.lock(id)?;
        self.machine(id, actor)?;
        self.require_stopped(id).await?;
        validate_id(snapshot_id)?;
        let snapshot = self
            .snapshots(id, actor)?
            .into_iter()
            .find(|s| s.id == snapshot_id)
            .context("Snapshot not found")?;
        let source = self
            .root
            .join("snapshots")
            .join(id)
            .join(&snapshot.id)
            .join("instance");
        let home = self.root.join("lima");
        let stage = home.join(format!(".restore-{}", model::new_uuid()));
        copy_tree(&source, &stage).await?;
        let current = home.join(id);
        let previous_snapshot = MachineSnapshot {
            id: model::new_uuid(),
            name: format!("Before restoring {}", snapshot.name),
            machine_id: id.into(),
        };
        let previous_dir = self
            .root
            .join("snapshots")
            .join(id)
            .join(&previous_snapshot.id);
        std::fs::create_dir_all(&previous_dir)?;
        store::json::write(&previous_dir.join("snapshot.json"), &previous_snapshot)?;
        let previous = previous_dir.join("instance");
        if let Err(error) = std::fs::rename(&current, &previous) {
            let _ = std::fs::remove_dir_all(&previous_dir);
            return Err(error.into());
        }
        if let Err(error) = std::fs::rename(&stage, &current) {
            std::fs::rename(&previous, &current).with_context(|| {
                format!(
                    "Could not recover the previous VM; its disk is retained at {}",
                    previous.display()
                )
            })?;
            let _ = std::fs::remove_dir_all(&previous_dir);
            return Err(error.into());
        }
        Ok(())
    }
}
