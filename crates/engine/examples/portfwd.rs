//! Does `-p 8080:80` actually reach localhost on the Mac?
//!
//! The whole point of the forwarder: boot the engine, run a published
//! container, sync forwards, and fetch the port from the host side.

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    use engine::vz::forward::wanted_forwards;
    use engine::vz::forwarder::Forwarder;
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
        4, 8 * 1024 * 1024 * 1024,
    );

    let machine = Arc::new(Machine::create(&layout, &resources, &shares::resolve(&[], |p| p.exists()), &[]).unwrap());
    machine.start().await.unwrap();
    let for_bridge = Arc::clone(&machine);
    let sock = socket.clone();
    tokio::spawn(async move { let _ = bridge::serve(for_bridge, sock).await; });

    let client = docker::Client::new(docker::Endpoint::Unix { path: socket.to_string_lossy().to_string() });
    for i in 1..=30 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if client.ping().await.is_ok() { println!("engine up after {}s", i * 2); break; }
    }

    // Pull and run nginx with a published port.
    println!("pulling nginx…");
    let _ = docker::images::pull(&client, "p", "nginx:alpine", |_| {}).await;
    let input = model::RunInput {
        image: "nginx:alpine".into(),
        name: Some("hopper-fwd-test".into()),
        ports: vec![model::PortMapping { host: "18080".into(), container: "80".into(), proto: None }],
        ..Default::default()
    };
    match docker::containers::run(&client, &input).await {
        Ok(id) => println!("container {}", &id[..12]),
        Err(e) => { println!("RUN FAILED: {e}"); let _ = machine.stop().await; return; }
    }

    // Sync the forwarder from what the daemon reports.
    tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    let containers = docker::containers::list(&client, false).await.unwrap_or_default();
    let wanted = wanted_forwards(&containers);
    println!("forwards wanted: {:?}", wanted.iter().map(|f| (f.host_port, f.guest_port)).collect::<Vec<_>>());

    let forwarder = Forwarder::new();
    let failures = forwarder.sync(&machine, wanted).await;
    for (f, why) in &failures { println!("BIND FAILED {}: {why}", f.host_port); }
    println!("active forwards: {}", forwarder.active().len());

    // The real test: fetch it from the host.
    tokio::time::sleep(std::time::Duration::from_secs(1)).await;
    match tokio::time::timeout(std::time::Duration::from_secs(10), async {
        let mut s = tokio::net::TcpStream::connect(("127.0.0.1", 18080)).await?;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        s.write_all(b"GET / HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n").await?;
        let mut buf = vec![0u8; 512];
        let n = s.read(&mut buf).await?;
        Ok::<_, std::io::Error>(String::from_utf8_lossy(&buf[..n]).to_string())
    }).await {
        Ok(Ok(text)) if text.contains("HTTP/1.1") => {
            println!("HOST FETCH OK: {}", text.lines().next().unwrap_or(""))
        }
        Ok(Ok(text)) => println!("HOST FETCH RETURNED NO HTTP ({} bytes)", text.len()),
        Ok(Err(e)) => println!("HOST FETCH FAILED: {e}"),
        Err(_) => println!("HOST FETCH TIMED OUT"),
    }

    let _ = docker::containers::remove(&client, "hopper-fwd-test", true, false).await;
    forwarder.shutdown();
    let _ = machine.stop().await;
    println!("done");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
