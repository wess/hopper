use engine::machines::{Actor, Machines};
use model::{CreateMachine, EngineResources};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let manager = Machines::default();
    let args: Vec<String> = std::env::args().skip(1).collect();
    let id = args.get(1).map(String::as_str).unwrap_or_default();
    match args.first().map(String::as_str) {
        Some("create") => {
            let machine = manager
                .create(CreateMachine {
                    name: args
                        .get(2)
                        .cloned()
                        .unwrap_or_else(|| "Hopper desktop".into()),
                    profile: id.into(),
                    resources: EngineResources {
                        cpus: 2,
                        memory_gib: 4,
                        disk_gib: if id == "ubuntu" { 24 } else { 64 },
                    },
                    installer: args.get(3).cloned(),
                    agent_access: true,
                })
                .await?;
            println!("{}", serde_json::to_string_pretty(&machine)?);
        }
        Some("start") => manager.start(id, Actor::Person).await?,
        Some("stop") => manager.stop(id, Actor::Person).await?,
        Some("exec") => println!("{}", manager.exec(id, Actor::Person, &args[2..]).await?),
        Some("screenshot") => {
            manager
                .screenshot(
                    id,
                    Actor::Person,
                    std::path::Path::new(args.get(2).expect("output path")),
                )
                .await?
        }
        Some("agent") => manager.set_agent_access(id, args.get(2).is_some_and(|v| v == "on"))?,
        _ => println!(
            "{}",
            serde_json::to_string_pretty(&manager.list(Actor::Person).await?)?
        ),
    }
    Ok(())
}
