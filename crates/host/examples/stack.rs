//! Drive a compose file through the real engine: plan, up, list, down.
//!
//! `cargo run -p host --example stack -- <dir> [down]`
use model::ComposeProgress;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().collect();
    let dir = args.get(1).cloned().unwrap_or_else(|| ".".into());
    let down_only = args.get(2).map(|s| s == "down").unwrap_or(false);

    let host = host::Host::from_env();
    let status = host.select_engine().await;
    println!("engine: {} ({})\n", status.provider, status.message);

    let plan = match host.compose_plan_path(&dir, &compose::PlanOptions::default()) {
        Ok(p) => p,
        Err(e) => {
            println!("PLAN FAILED: {e}");
            return;
        }
    };
    println!("project  = {}", plan.project);
    println!("networks = {:?}", plan.networks.iter().map(|n| &n.name).collect::<Vec<_>>());
    println!("volumes  = {:?}", plan.volumes.iter().map(|v| &v.name).collect::<Vec<_>>());
    for s in &plan.services {
        println!("\nservice {} (selected={} blocked={:?})", s.service, s.selected, s.blocked);
        println!("  name   {:?}", s.run.name);
        println!("  image  {}", s.run.image);
        println!("  net    {:?} extra={:?}", s.run.network, s.extra_networks);
        println!("  env    {:?}", s.run.env);
        println!("  ports  {:?}", s.run.ports.iter().map(|p| format!("{}->{}", p.host, p.container)).collect::<Vec<_>>());
        println!("  mounts {:?}", s.run.volumes.iter().map(|v| format!("{}:{}{}", v.host, v.container, if v.ro {":ro"} else {""})).collect::<Vec<_>>());
        println!("  after  {:?}", s.depends_on);
        for w in &s.warnings { println!("  ! {w}"); }
    }
    println!("\n--- {} ---", if down_only { "down" } else { "up" });
    let mut sink = |p: ComposeProgress| {
        let mark = if matches!(p.stream, model::StreamKind::Stderr) { "!" } else { " " };
        println!("{mark} {}", p.line);
    };
    if down_only {
        host.compose_down(&plan.project, true, &mut sink).await;
        return;
    }
    host.compose_up(&plan, &mut sink).await;

    println!("\n--- stacks ---");
    for p in host.compose_projects().await.unwrap_or_default() {
        println!("{} {}/{} {:?}", p.name, p.running, p.total, p.service_names());
        println!("  files {:?}", p.config_files);
    }
}
