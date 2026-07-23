//! First-boot harness for the managed engine.
//!
//! Builds the VM against the guest assets in native/build/guest, boots it, and
//! reports whether dockerd inside answers. Must be signed with
//! com.apple.security.virtualization to run.
//!
//! cargo build -p engine --example boot && \
//!   codesign --force --entitlements assets/hopper.entitlements -s - target/debug/examples/boot && \
//!   target/debug/examples/boot

#[cfg(target_os = "macos")]
#[tokio::main]
async fn main() {
    use engine::vz::machine::Machine;
    use engine::vz::{shares, vm};
    use model::EngineResources;
    use std::path::PathBuf;

    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf();
    let assets = root.join("native/build/guest");
    let state = root.join("native/build/vmstate");

    let layout = vm::Layout::resolve(&assets, &state);
    println!("kernel : {}", layout.kernel.display());
    println!("initrd : {}", layout.initrd.display());
    println!("disk   : {}", layout.disk.display());
    println!("vz supported: {}", vm::supported());

    let missing = layout.missing(|p| p.exists());
    if !missing.is_empty() {
        println!("MISSING: {}", missing.join(", "));
        return;
    }

    let resources = vm::clamp(
        EngineResources {
            cpus: 2,
            memory_gib: 2,
            disk_gib: 16,
        },
        std::thread::available_parallelism().map(|n| n.get() as u32).unwrap_or(4),
        8 * 1024 * 1024 * 1024,
    );
    let share_list = shares::resolve(&[], |p| p.exists());
    println!("config : {}", vm::describe(&resources, &share_list));

    let machine = match Machine::create(&layout, &resources, &share_list, &[]) {
        Ok(m) => m,
        Err(e) => {
            println!("CREATE FAILED: {e}");
            return;
        }
    };
    println!("created, starting…");

    match machine.start().await {
        Ok(()) => println!("STARTED: phase={:?}", machine.phase()),
        Err(e) => {
            println!("START FAILED: {e}");
            return;
        }
    }

    // Give the guest time to bring dockerd up, then try the vsock port.
    for attempt in 1..=20 {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        match machine.connect(vm::DOCKER_VSOCK_PORT).await {
            Ok(_) => {
                println!("VSOCK OK after {}s — dockerd is reachable", attempt * 2);
                break;
            }
            Err(e) => {
                if attempt % 5 == 0 {
                    println!("  ({}s) not yet: {e}", attempt * 2);
                }
                if attempt == 20 {
                    println!("VSOCK NEVER CAME UP");
                }
            }
        }
    }

    println!("stopping…");
    let _ = machine.stop().await;
    println!("done: phase={:?}", machine.phase());
}

#[cfg(not(target_os = "macos"))]
fn main() {
    println!("macOS only");
}
