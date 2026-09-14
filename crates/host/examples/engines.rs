//! What engines this machine offers, and what each one answers when pinned.
//!
//! `cargo run -p host --example engines`
#[tokio::main]
async fn main() {
    let host = host::Host::from_env();

    println!("--- offered ---");
    for c in host.engine_choices().await {
        let where_ = c
            .reason
            .clone()
            .or_else(|| c.endpoint.clone())
            .unwrap_or_default();
        println!(
            "  {:<18} {:<10} {}",
            c.label,
            if c.available { "available" } else { "-" },
            where_
        );
    }

    println!("\n--- pinned ---");
    for id in ["apple", "docker", "podman", "colima", "existing"] {
        let status = host.set_engine_preference(Some(id.to_string())).await;
        match status {
            Ok(status) => println!(
                "  {:<10} -> provider={:<10} connected={:<5} {}",
                id, status.provider, status.connected, status.message
            ),
            Err(error) => println!("  {id:<10} -> could not persist preference: {error}"),
        }
    }

    // Hand selection back so the example leaves nothing pinned.
    let status = host.set_engine_preference(None).await;
    match status {
        Ok(status) => println!("\nautomatic -> {} ({})", status.provider, status.message),
        Err(error) => println!("\nautomatic -> could not clear preference: {error}"),
    }
}
