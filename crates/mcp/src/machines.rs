use base64::{engine::general_purpose::STANDARD, Engine};
use host::{Host, MachineActor};
use model::{CreateMachine, EngineResources, MachineInput};
use serde_json::{json, Value};
use std::sync::Arc;

pub fn catalogue() -> Vec<Value> {
    let id = json!({"type":"string","description":"VM id from vm.list."});
    let string = json!({"type":"string"});
    [
        ("vm.profiles", "List downloadable guest OS profiles and their requirements.", json!({}), vec![]),
        ("vm.list", "List VMs that permit agent access. VMs with access disabled are excluded.", json!({}), vec![]),
        ("vm.create", "Create a Linux, macOS, or Windows VM. Windows uses the native runtime; its remote lifecycle and guest tools are not available yet. Linux and macOS images download on vm.start. New VMs enable agent access by default; set agentAccess=false to disable it.",json!({"name":string,"profile":string,"cpus":{"type":"integer","minimum":1,"maximum":64},"memoryGiB":{"type":"integer","minimum":1,"maximum":256},"diskGiB":{"type":"integer","minimum":10,"maximum":2048},"agentAccess":{"type":"boolean"}}),vec!["name","profile"]),
        ("vm.start", "Start a VM and open its dedicated display window. First boot downloads and prepares the guest and can take several minutes. Windows startup currently requires the Hopper app.",json!({"id":id}),vec!["id"]),
        ("vm.stop", "Gracefully stop a VM; its disk is retained.",json!({"id":id}),vec!["id"]),
        ("vm.exec", "Execute a command inside the guest, never on the host. argv is an array of command arguments, such as [\"uname\",\"-a\"]. Output is bounded and commands time out after 120 seconds.",json!({"id":id,"argv":{"type":"array","items":{"type":"string"},"minItems":1}}),vec!["id","argv"]),
        ("vm.screenshot", "Capture the VM display as a PNG image. The VM must be running with its display visible.",json!({"id":id}),vec!["id"]),
        ("vm.input", "Send guest input. Linux uses xdotool key names. Native Windows remote input is not available yet. Pointer x/y are normalized 0–32767. macOS uses guest key names (super is Command) and requires guest Accessibility permission for osascript. Windows guest tools are pending.",json!({"id":id,"input":{"oneOf":[{"type":"object","properties":{"type":{"const":"key"},"keys":string},"required":["type","keys"]},{"type":"object","properties":{"type":{"const":"text"},"text":string},"required":["type","text"]},{"type":"object","properties":{"type":{"const":"pointer"},"x":{"type":"integer","minimum":0,"maximum":32767},"y":{"type":"integer","minimum":0,"maximum":32767},"button":{"enum":["left","right","middle"]}},"required":["type","x","y"]}]}}),vec!["id","input"]),
        ("vm.read_file", "Read a UTF-8 file from the guest (up to 1 MiB).",json!({"id":id,"path":string}),vec!["id","path"]),
        ("vm.write_file", "Write UTF-8 content into a guest file (up to 1 MiB). Overwrites the destination. No host folders are shared with these VMs.",json!({"id":id,"path":string,"content":string}),vec!["id","path","content"]),
        ("vm.clone", "Clone a stopped VM. The clone inherits the source's agent setting.",json!({"id":id,"name":string}),vec!["id","name"]),
        ("vm.snapshots", "List disk snapshots of a VM.",json!({"id":id}),vec!["id"]),
        ("vm.snapshot", "Create an offline disk snapshot. Stop the VM first. Requires macOS and APFS.",json!({"id":id,"name":string}),vec!["id","name"]),
        ("vm.restore", "Restore an offline disk snapshot. Stop the VM first. The replaced disk is retained for recovery.",json!({"id":id,"snapshot":string}),vec!["id","snapshot"]),
    ].into_iter().map(|(name,description,properties,required)|json!({"name":name,"description":description,"inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}})).collect()
}

fn arg<'a>(args: &'a Value, key: &str) -> anyhow::Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| anyhow::anyhow!("Provide `{key}`"))
}

fn resource(args: &Value, key: &str, default: u32) -> anyhow::Result<u32> {
    match args.get(key) {
        None => Ok(default),
        Some(value) => value
            .as_u64()
            .and_then(|n| n.try_into().ok())
            .ok_or_else(|| anyhow::anyhow!("`{key}` must be a positive integer")),
    }
}

pub async fn call(host: &Arc<Host>, name: &str, args: &Value) -> Value {
    match run(host, name, args).await {
        Ok(result) => result,
        Err(error) => crate::protocol::error_result(format!("{error:#}")),
    }
}

async fn run(host: &Arc<Host>, name: &str, args: &Value) -> anyhow::Result<Value> {
    let machines = host.machines();
    let actor = MachineActor::Agent;
    let text = crate::protocol::text_result;
    if name == "vm.profiles" {
        return Ok(text(serde_json::to_string(&machines.profiles())?));
    }
    if name == "vm.list" {
        return Ok(text(serde_json::to_string(&host.list_machines(actor).await?)?));
    }
    if name == "vm.create" {
        let profile = arg(args, "profile")?;
        let windows = machines.profiles().iter().any(|entry| {
            entry.id == profile && entry.guest == model::GuestOs::Windows
        });
        let machine = host
            .create_machine(CreateMachine {
                name: arg(args, "name")?.into(),
                profile: profile.into(),
                resources: EngineResources {
                    cpus: resource(args, "cpus", if windows { 2 } else { 4 })?,
                    memory_gib: resource(args, "memoryGiB", 4)?,
                    disk_gib: resource(args, "diskGiB", 64)?,
                },
                installer: None,
                agent_access: match args.get("agentAccess") {
                    None => true,
                    Some(v) => v
                        .as_bool()
                        .ok_or_else(|| anyhow::anyhow!("`agentAccess` must be a boolean"))?,
                },
            })
            .await?;
        return Ok(text(serde_json::to_string(&machine)?));
    }
    let id = arg(args, "id")?;
    // Authorize before creating temporary files or revealing VM metadata.
    let machine = machines.machine(id, actor)?;
    if machine.guest == model::GuestOs::Windows {
        anyhow::bail!(
            "Native Windows remote operations are not available yet. Use the Hopper app; this operation will not start the previous runtime."
        );
    }
    match name {
        "vm.start" => machines.start(id, actor).await?,
        "vm.stop" => machines.stop(id, actor).await?,
        "vm.exec" => {
            let argv: Vec<String> = serde_json::from_value(
                args.get("argv")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Provide `argv`"))?,
            )?;
            return Ok(text(machines.exec(id, actor, &argv).await?));
        }
        "vm.input" => {
            let input: MachineInput = serde_json::from_value(
                args.get("input")
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("Provide `input`"))?,
            )?;
            machines.input(id, actor, input).await?;
        }
        "vm.screenshot" => {
            let dir = tempfile::tempdir()?;
            let path = dir.path().join("display.png");
            machines.screenshot(id, actor, &path).await?;
            let size = std::fs::metadata(&path)?.len();
            if size > 16 * 1024 * 1024 {
                anyhow::bail!("VM screenshot exceeds 16 MiB");
            }
            let bytes = std::fs::read(path)?;
            if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
                anyhow::bail!("VM helper did not produce a PNG screenshot");
            }
            return Ok(
                json!({"content":[{"type":"image","mimeType":"image/png","data":STANDARD.encode(bytes)}]}),
            );
        }
        "vm.read_file" | "vm.write_file" => {
            let dir = tempfile::tempdir()?;
            let file = dir.path().join("transfer");
            let upload = name == "vm.write_file";
            if upload {
                let content = args
                    .get("content")
                    .and_then(Value::as_str)
                    .ok_or_else(|| anyhow::anyhow!("Provide `content`"))?;
                if content.len() > 1024 * 1024 {
                    anyhow::bail!("Guest file content is limited to 1 MiB");
                }
                std::fs::write(&file, content)?;
            }
            machines
                .copy(id, actor, &file, arg(args, "path")?, upload)
                .await?;
            if !upload {
                if std::fs::metadata(&file)?.len() > 1024 * 1024 {
                    anyhow::bail!("Guest file exceeds 1 MiB");
                }
                return Ok(text(std::fs::read_to_string(file)?));
            }
        }
        "vm.clone" => {
            return Ok(text(serde_json::to_string(
                &machines
                    .clone_machine(id, actor, arg(args, "name")?)
                    .await?,
            )?))
        }
        "vm.snapshots" => {
            return Ok(text(serde_json::to_string(
                &machines.snapshots(id, actor)?,
            )?))
        }
        "vm.snapshot" => {
            return Ok(text(serde_json::to_string(
                &machines.snapshot(id, actor, arg(args, "name")?).await?,
            )?))
        }
        "vm.restore" => machines.restore(id, actor, arg(args, "snapshot")?).await?,
        _ => anyhow::bail!("Unknown VM tool: {name}"),
    }
    Ok(text("Done".to_string()))
}
