//! Live lifecycle check: cargo run -p engine --example vm -- start|stop|status.

use engine::{providers::vm::Vm, Provider};
use model::EngineResources;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let vm = Vm::default();
    match std::env::args().nth(1).as_deref() {
        Some("start") => vm.start(EngineResources::default()).await?,
        Some("stop") => vm.stop().await?,
        Some("status") | None => {}
        _ => anyhow::bail!("expected start, stop, or status"),
    }
    println!("{}", serde_json::to_string_pretty(&vm.status().await)?);
    Ok(())
}
