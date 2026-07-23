//! Does a bind mount outside $HOME actually reach the container?
//!
//! The Swift build shared only the home directory, so `-v /opt/data:/data`
//! silently produced an empty directory. This proves an extra share works.

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
    let resources = vm::clamp(EngineResources { cpus: 2, memory_gib: 2, disk_gib: 16 }, 4, 8 * 1024 * 1024 * 1024);

    // The whole point: a directory that is NOT under $HOME.
    let share_list = shares::resolve(&["/tmp/hoppershare".into()], |p| p.exists());
    println!("shares: {:?}", share_list.iter().map(|s| s.path.display().to_string()).collect::<Vec<_>>());
    let cmdline = vm::share_args(&share_list);
    println!("cmdline: {cmdline:?}");

    let machine = Arc::new(Machine::create(&layout, &resources, &share_list, &cmdline).unwrap());
    machine.start().await.unwrap();
    let fb = Arc::clone(&machine); let sk = socket.clone();
    tokio::spawn(async move { let _ = bridge::serve(fb, sk).await; });

    let client = docker::Client::new(docker::Endpoint::Unix { path: socket.to_string_lossy().to_string() });
    for i in 1..=30 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        if client.ping().await.is_ok() { println!("engine up after {}s", i * 2); break; }
    }

    let _ = docker::images::pull(&client, "p", "alpine:latest", |_| {}).await;
    let input = model::RunInput {
        image: "alpine:latest".into(),
        name: Some("hopper-share-test".into()),
        command: Some("cat /mnt/proof.txt".into()),
        volumes: vec![model::VolumeMapping {
            host: "/tmp/hoppershare".into(),
            container: "/mnt".into(),
            ro: false,
        }],
        ..Default::default()
    };
    match docker::containers::run(&client, &input).await {
        Ok(id) => {
            tokio::time::sleep(std::time::Duration::from_secs(3)).await;
            match docker::logs::snapshot(&client, &id, 20).await {
                Ok(lines) => {
                    let text: String = lines.iter().map(|l| l.text.clone()).collect::<Vec<_>>().join("");
                    if text.contains("file-sharing-works") {
                        println!("SHARE OK — container read the host file: {}", text.trim());
                    } else {
                        println!("SHARE FAILED — container saw: {:?}", text.trim());
                    }
                }
                Err(e) => println!("LOGS FAILED: {e}"),
            }
            let _ = docker::containers::remove(&client, &id, true, false).await;
        }
        Err(e) => println!("RUN FAILED: {e}"),
    }
    let _ = machine.stop().await;
    println!("done");
}

#[cfg(not(target_os = "macos"))]
fn main() {}
