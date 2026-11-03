#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod probe {
  use anyhow::{ensure, Context};
  use engine::machines::{vz::Action, Actor};
  use futures::StreamExt;
  use host::Host;
  use machine::vz::{self, queue::Owner, Boot, Linux};
  use objc2_foundation::{NSDate, NSDefaultRunLoopMode, NSRunLoop};
  use serde_json::{json, Value};
  use sha2::{Digest, Sha256};
  use std::{
    os::unix::fs::OpenOptionsExt,
    path::Path,
    sync::Arc,
    time::{Duration, Instant},
  };
  use tokio::{
    io::AsyncWriteExt,
    process::{Child, ChildStdin},
    runtime::Runtime,
  };
  use tokio_util::codec::{FramedRead, LinesCodec};

  struct Agent {
    child: Child,
    input: ChildStdin,
    output: FramedRead<tokio::process::ChildStdout, LinesCodec>,
    sequence: u64,
  }

  impl Agent {
    fn open(root: &Path) -> anyhow::Result<Self> {
      let mut child = tokio::process::Command::new(std::env::current_exe()?)
        .arg("--stdio")
        .env("HOPPER_DIR", root)
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
      let input = child.stdin.take().context("Missing MCP stdin")?;
      let output = FramedRead::new(
        child.stdout.take().context("Missing MCP stdout")?,
        LinesCodec::new_with_max_length(1 << 20),
      );
      Ok(Self {
        child,
        input,
        output,
        sequence: 0,
      })
    }

    async fn call(&mut self, name: &str, arguments: Value) -> anyhow::Result<Value> {
      self
        .rpc("tools/call", json!({"name":name,"arguments":arguments}))
        .await
    }

    async fn rpc(&mut self, method: &str, params: Value) -> anyhow::Result<Value> {
      self.sequence += 1;
      let request = json!({"jsonrpc":"2.0", "id":self.sequence, "method":method, "params":params});
      self
        .input
        .write_all(format!("{request}\n").as_bytes())
        .await?;
      let line = tokio::time::timeout(Duration::from_secs(195), self.output.next())
        .await?
        .context("MCP stdout closed")??;
      let reply: Value = serde_json::from_str(&line)?;
      ensure!(
        reply["id"] == self.sequence && reply.get("error").is_none(),
        "Invalid MCP reply: {reply}"
      );
      Ok(reply["result"].clone())
    }

    async fn value(&mut self, name: &str, arguments: Value) -> anyhow::Result<Value> {
      let result = self.call(name, arguments).await?;
      ensure!(result["isError"] != true, "MCP operation failed: {result}");
      serde_json::from_str(
        result["content"][0]["text"]
          .as_str()
          .context("Missing tool result")?,
      )
      .map_err(Into::into)
    }

    async fn finish(
      &mut self,
      id: &str,
      mut job: model::guest::Job,
    ) -> anyhow::Result<model::guest::Job> {
      let deadline = Instant::now() + Duration::from_secs(45);
      while !job.finished {
        ensure!(Instant::now() < deadline, "MCP guest job timed out");
        tokio::time::sleep(Duration::from_millis(25)).await;
        job = serde_json::from_value(
          self
            .value("vm.job_poll", json!({"id":id,"job":job.id}))
            .await?,
        )?;
      }
      ensure!(
        !job.stdout_truncated && !job.stderr_truncated && job.cleanup_error.is_none(),
        "Incomplete guest result"
      );
      Ok(job)
    }

    async fn command(
      &mut self,
      id: &str,
      executable: &str,
      args: Vec<&str>,
      role: &str,
    ) -> anyhow::Result<model::guest::Job> {
      let job = serde_json::from_value(self.value("vm.job_start", json!({"id":id,"command":{"executable":executable,"args":args,"role":role,"timeoutSeconds":150}})).await?)?;
      self.finish(id, job).await
    }
  }

  fn wait<T: Send + 'static>(
    runtime: &Runtime,
    owner: &mut Owner,
    console: &Path,
    future: impl std::future::Future<Output = anyhow::Result<T>> + Send + 'static,
  ) -> anyhow::Result<T> {
    let task = runtime.spawn(future);
    let deadline = Instant::now() + Duration::from_secs(210);
    let run_loop = NSRunLoop::currentRunLoop();
    while !task.is_finished() {
      ensure!(Instant::now() < deadline, "Agent diagnostic timed out");
      ensure!(
        std::fs::metadata(console)?.len() <= 1 << 20,
        "Agent diagnostic console exceeds bounds"
      );
      owner.tick();
      run_loop.runMode_beforeDate(
        unsafe { NSDefaultRunLoopMode },
        &NSDate::dateWithTimeIntervalSinceNow(0.01),
      );
    }
    runtime.block_on(task)?
  }

  pub(super) async fn exercise(
    host: Arc<Host>,
    root: std::path::PathBuf,
    id: &'static str,
  ) -> anyhow::Result<()> {
    let mut agent = Agent::open(&root)?;
    let initialized = agent
      .rpc(
        "initialize",
        json!({"protocolVersion":mcp::protocol::PROTOCOL_VERSION,
      "capabilities":{},"clientInfo":{"name":"hopper-guest-diagnostic","version":"1"}}),
      )
      .await?;
    ensure!(
      initialized["protocolVersion"] == mcp::protocol::PROTOCOL_VERSION,
      "MCP initialization mismatch"
    );
    agent
      .input
      .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
      .await?;
    for (role, uid) in [("normal", 1001), ("administrator", 0)] {
      let job = agent.command(id, "/usr/bin/id", vec!["-u"], role).await?;
      ensure!(
        job.exit_code == Some(0)
          && job.stdout == format!("{uid}\n").as_bytes()
          && job.identity.is_some_and(|identity| identity.uid == uid),
        "MCP guest identity mismatch"
      );
    }
    let bytes = vec![255; model::guest::files::CREATE_LIMIT];
    let job = serde_json::from_value(
      agent
        .value(
          "vm.file_create",
          json!({"id":id,"file":{"path":"/home/hopper/mcp.bin","bytes":bytes,"role":"normal"}}),
        )
        .await?,
    )?;
    let job = agent.finish(id, job).await?;
    ensure!(job.exit_code == Some(0), "MCP guest file creation failed");
    let receipt: model::guest::files::Receipt = serde_json::from_slice(&job.stdout)?;
    let job = serde_json::from_value(agent.value("vm.file_read",json!({"id":id,"file":{"path":"/home/hopper/mcp.bin","offset":0,"length":32768,"stamp":receipt.stamp,"role":"normal"}})).await?)?;
    let job = agent.finish(id, job).await?;
    ensure!(
      job.exit_code == Some(0) && job.stdout == bytes,
      "MCP binary round-trip failed"
    );
    let job = serde_json::from_value(
      agent
        .value(
          "vm.file_browse",
          json!({"id":id,"directory":{"path":"/home/hopper","limit":32,"role":"normal"}}),
        )
        .await?,
    )?;
    let job = agent.finish(id, job).await?;
    let listing: model::guest::files::Listing = serde_json::from_slice(&job.stdout)?;
    ensure!(
      job.exit_code == Some(0) && listing.entries.iter().any(|entry| entry.name == b"mcp.bin"),
      "MCP directory browse failed"
    );
    streaming(&mut agent, id).await?;
    let running: model::guest::Job = serde_json::from_value(agent.value("vm.job_start",json!({"id":id,"command":{"executable":"/bin/sh","args":["-c","echo $$ > /home/hopper/revoke.pid; exec sleep 120"],"role":"normal","timeoutSeconds":150}})).await?)?;
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
      let job: model::guest::Job = serde_json::from_value(
        agent
          .value("vm.job_poll", json!({"id":id,"job":running.id}))
          .await?,
      )?;
      if job.identity.is_some() {
        break;
      }
      ensure!(
        Instant::now() < deadline && !job.finished,
        "Revocation job did not launch"
      );
    }
    host.machines().set_agent_access(id, false)?;
    let denied = agent
      .call("vm.job_poll", json!({"id":id,"job":running.id}))
      .await?;
    ensure!(
      denied["isError"] == true,
      "Revoked MCP request was authorized"
    );
    tokio::time::sleep(Duration::from_millis(300)).await;
    host.machines().set_agent_access(id, true)?;
    let stale = host
      .request_guest_job(
        id,
        0,
        model::guest::wire::Action::Poll {
          job: running.id.clone(),
        },
      )
      .await;
    ensure!(stale.is_err(), "Old policy generation was refreshed");
    let job = agent
      .command(
        id,
        "/bin/sh",
        vec![
          "-c",
          "pid=$(cat /home/hopper/revoke.pid); test ! -e /proc/$pid",
        ],
        "normal",
      )
      .await?;
    ensure!(
      job.exit_code == Some(0),
      "Revocation left the guest process running"
    );
    let old = agent
      .call("vm.job_poll", json!({"id":id,"job":running.id}))
      .await?;
    ensure!(
      old["isError"] == true,
      "Old guest session history survived revocation"
    );
    agent.child.kill().await?;
    agent.child.wait().await?;
    Ok(())
  }

  async fn streaming(agent: &mut Agent, id: &str) -> anyhow::Result<()> {
    use model::guest::files::{Transfer, Uploaded, CHUNK, CREATE_LIMIT};
    let bytes = (0..3 * CREATE_LIMIT + 17)
      .map(|index| index as u8)
      .collect::<Vec<_>>();
    let digest = format!("{:x}", Sha256::digest(&bytes));
    let mut transfer: Transfer = serde_json::from_value(agent.value("vm.upload_begin",json!({"id":id,"file":{
      "path":"/home/hopper/stream.bin","size":bytes.len(),"sha256":digest,"role":"normal","timeoutSeconds":90
    }})).await?)?;
    let deadline = Instant::now() + Duration::from_secs(90);
    while transfer.offset < transfer.size {
      ensure!(
        Instant::now() < deadline && !transfer.job.finished,
        "MCP upload stalled or failed"
      );
      let offset = transfer.offset as usize;
      let end = bytes.len().min(offset + CREATE_LIMIT);
      transfer = serde_json::from_value(
        agent
          .value(
            "vm.upload_chunk",
            json!({"id":id,"job":transfer.job.id,
        "offset":offset,"bytes":bytes[offset..end]}),
          )
          .await?,
      )?;
      ensure!(
        transfer.offset >= offset as u64 && transfer.offset <= end as u64,
        "MCP upload offset mismatch"
      );
      if transfer.offset == offset as u64 {
        tokio::time::sleep(Duration::from_millis(25)).await;
      }
    }
    let status: Transfer = serde_json::from_value(
      agent
        .value("vm.upload_status", json!({"id":id,"job":transfer.job.id}))
        .await?,
    )?;
    ensure!(
      status.offset == transfer.size && !status.committed,
      "MCP upload committed without request"
    );
    while !transfer.committed {
      ensure!(Instant::now() < deadline, "MCP upload commit stalled");
      transfer = serde_json::from_value(
        agent
          .value("vm.upload_commit", json!({"id":id,"job":transfer.job.id}))
          .await?,
      )?;
      if !transfer.committed {
        tokio::time::sleep(Duration::from_millis(25)).await;
      }
    }
    let job = agent.finish(id, transfer.job).await?;
    ensure!(job.exit_code == Some(0), "MCP upload failed after commit");
    let receipt: Uploaded = serde_json::from_slice(&job.stdout)?;
    ensure!(
      receipt.sha256 == digest && receipt.file.length == bytes.len(),
      "MCP upload receipt mismatch"
    );
    let mut offset = 0;
    while offset < bytes.len() {
      let job = serde_json::from_value(agent.value("vm.file_read",json!({"id":id,"file":{
        "path":"/home/hopper/stream.bin","offset":offset,"length":CHUNK,"stamp":receipt.file.stamp,"role":"normal"
      }})).await?)?;
      let job = agent.finish(id, job).await?;
      let end = bytes.len().min(offset + CHUNK);
      ensure!(
        job.exit_code == Some(0) && job.stdout == bytes[offset..end],
        "MCP streaming binary readback mismatch"
      );
      offset = end;
    }
    Ok(())
  }

  pub fn run() -> anyhow::Result<()> {
    if std::env::args().nth(1).as_deref() == Some("--stdio") {
      return Runtime::new()?.block_on(mcp::serve::run(Host::from_env()));
    }
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    ensure!(
      args.len() == 3,
      "Provide verified ARM64 kernel, production helper initramfs and modloop"
    );
    let root = tempfile::tempdir_in("/tmp")?;
    std::env::set_var("HOPPER_DIR", root.path());
    let host = Host::from_env();
    std::env::remove_var("HOPPER_DIR");
    let runtime = Runtime::new()?;
    let id = "8b748c83-390b-4819-9665-ea2a67a48930";
    let manager = host.machines();
    std::fs::create_dir_all(manager.root.join("records"))?;
    let record = model::Machine {
      id: id.into(),
      name: "Isolated Alpine agent diagnostic".into(),
      guest: model::GuestOs::Linux,
      profile: "ubuntu".into(),
      resources: model::EngineResources {
        cpus: 2,
        memory_gib: 1,
        disk_gib: 64,
      },
      installer: None,
      agent_access: true,
      agent_generation: 0,
      runtime: Some(model::MachineRuntime::Virtualization),
    };
    std::fs::write(
      manager.root.join("records").join(format!("{id}.json")),
      serde_json::to_vec(&record)?,
    )?;
    let main = vz::MainThreadMarker::new().context("Agent probe requires main thread")?;
    let mut owner = host.virtual_machine_owner()?;
    let disk = root.path().join("disk");
    std::fs::OpenOptions::new()
      .create_new(true)
      .write(true)
      .mode(0o600)
      .open(&disk)?
      .set_len(1 << 30)?;
    let output = root.path().join("console");
    let console = std::fs::OpenOptions::new()
      .create_new(true)
      .write(true)
      .mode(0o600)
      .open(&output)?;
    let linux = Linux {
      cpus: 2,
      memory: 512 << 20,
      width: 1024,
      height: 768,
      identity: vz::identity(),
      boot: Boot::Kernel {
        kernel: args[0].clone().into(),
        initramfs: Some(args[1].clone().into()),
        command_line: "console=hvc0 rdinit=/init".into(),
      },
      disk,
      installer: Some(args[2].clone().into()),
      seed: None,
      network: None,
      console: Some(console),
      shares: Vec::new(),
      speakers: false,
    };
    let check: machine::vz::queue::Check = Arc::new(|| Ok(()));
    owner.insert(
      id,
      vz::bootstrap::provision(main, &linux, id, owner.next_generation()?, &check)?,
    )?;
    {
      let _enter = runtime.enter();
      host.serve_machine_agents()?;
    }
    let start = host.clone();
    wait(&runtime, &mut owner, &output, async move {
      start
        .virtual_machine_lifecycle(id, Actor::Person, Action::Start)
        .await
    })?;
    let result = wait(
      &runtime,
      &mut owner,
      &output,
      exercise(host.clone(), root.path().to_owned(), id),
    );
    let stop = host.clone();
    let stopped = wait(&runtime, &mut owner, &output, async move {
      stop
        .virtual_machine_lifecycle(id, Actor::Person, Action::Stop)
        .await
    });
    if result.is_err() || stopped.is_err() {
      eprintln!("Agent probe evidence retained at {}", root.keep().display());
    }
    result?;
    stopped?;
    owner.retire(id)?;
    println!("MCP stdio, private socket, guest identities/files/streaming/browsing and persisted-policy revocation verified");
    Ok(())
  }
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
#[path = "guestagent/ubuntu.rs"]
mod ubuntu;

fn main() -> anyhow::Result<()> {
  #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
  {
    if std::env::args().nth(1).as_deref() == Some("--ubuntu") {
      return ubuntu::run();
    }
    probe::run()
  }
  #[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
  anyhow::bail!("Guest agent probe requires Apple silicon macOS")
}
