//! Boot the managed engine, serve its socket, and drive it with the real
//! Docker API — the end-to-end proof that Hopper can *be* the engine.

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    use engine::vz::machine::Machine;
    use engine::vz::{bridge, shares, vm};
    use model::EngineResources;
    use std::path::PathBuf;
    use std::sync::Arc;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
    let layout = vm::Layout::resolve(&root.join("native/build/guest"), &root.join("native/build/vmstate"));
    let socket = root.join("native/build/vmstate/docker.sock");

    let resources = vm::clamp(
        EngineResources { cpus: 2, memory_gib: 2, disk_gib: 16 },
        std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4),
        8 * 1024 * 1024 * 1024,
    );
    let machine = Arc::new(Machine::create(&layout, &resources, &shares::resolve(&[], |p| p.exists()), &[]).unwrap());
    machine.start().await.unwrap();
    println!("engine started");

    let for_bridge = Arc::clone(&machine);
    let sock = socket.clone();
    tokio::spawn(async move { let _ = bridge::serve(for_bridge, sock).await; });

    // Wait for dockerd behind the bridge.
    let client = docker::Client::new(docker::Endpoint::Unix { path: socket.to_string_lossy().to_string() });
    for i in 1..=30 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if client.ping().await.is_ok() { println!("ping OK after {}s", i * 2); break; }
        if i == 30 { println!("NEVER PINGED"); }
    }

    match docker::system::version(&client).await {
        Ok(v) => println!("daemon : Docker {} (api {}) on {}/{}", v.version, v.api_version, v.os, v.arch),
        Err(e) => println!("version FAILED: {e}"),
    }
    match docker::system::info(&client).await {
        Ok(i) => println!("info   : {} containers, {} images, {} CPUs", i.containers, i.images, i.ncpu),
        Err(e) => println!("info FAILED: {e}"),
    }

    println!("socket : {}", socket.display());
    println!("HOLDING 90s — drive it with: DOCKER_HOST=unix://{} docker version", socket.display());
    tokio::time::sleep(std::time::Duration::from_secs(90)).await;
    let _ = machine.stop().await;
    println!("stopped");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
