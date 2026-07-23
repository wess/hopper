//! Does data actually flow through a connected vsock stream?
//!
//! The earlier boot harness only checked that `connect()` returned Ok — it
//! never wrote a byte. This speaks real HTTP to dockerd through the stream.

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    use engine::vz::machine::Machine;
    use engine::vz::{shares, vm};
    use model::EngineResources;
    use std::path::PathBuf;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).parent().unwrap().parent().unwrap().to_path_buf();
    let layout = vm::Layout::resolve(&root.join("native/build/guest"), &root.join("native/build/vmstate"));
    let resources = vm::clamp(
        EngineResources { cpus: 2, memory_gib: 2, disk_gib: 16 },
        4, 8 * 1024 * 1024 * 1024,
    );
    let machine = Machine::create(&layout, &resources, &shares::resolve(&[], |p| p.exists()), &[]).unwrap();
    machine.start().await.unwrap();
    println!("started");

    let mut stream = None;
    for i in 1..=30 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if let Ok(s) = machine.connect(vm::DOCKER_VSOCK_PORT).await {
            println!("connected at {}s", i * 2);
            stream = Some(s);
            break;
        }
    }
    let Some(mut stream) = stream else {
        println!("NEVER CONNECTED");
        let _ = machine.stop().await;
        return;
    };

    // The real test: speak HTTP and read the answer back.
    let request = b"GET /_ping HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n";
    match stream.write_all(request).await {
        Ok(()) => println!("wrote {} bytes", request.len()),
        Err(e) => { println!("WRITE FAILED: {e}"); let _ = machine.stop().await; return; }
    }
    let _ = stream.flush().await;

    let mut buf = vec![0u8; 1024];
    match tokio::time::timeout(std::time::Duration::from_secs(10), stream.read(&mut buf)).await {
        Ok(Ok(0)) => println!("READ RETURNED 0 — peer closed without answering"),
        Ok(Ok(n)) => println!("READ OK ({n} bytes): {:?}", String::from_utf8_lossy(&buf[..n.min(120)])),
        Ok(Err(e)) => println!("READ FAILED: {e}"),
        Err(_) => println!("READ TIMED OUT — data never arrived"),
    }

    let _ = machine.stop().await;
    println!("stopped");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
