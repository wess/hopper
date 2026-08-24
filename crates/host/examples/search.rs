//! Search every public registry Hopper knows, against the live APIs.
//!
//! `cargo run -p host --example search -- <query>`
use model::RegistrySource;

#[tokio::main]
async fn main() {
    let query = std::env::args().nth(1).unwrap_or_else(|| "nginx".into());
    for source in [
        RegistrySource::DockerHub,
        RegistrySource::Ghcr,
        RegistrySource::Quay,
    ] {
        println!("\n--- {} ---", source.label());
        match host::registry::search(source, &query).await {
            Ok(hits) if hits.is_empty() => println!("no results"),
            Ok(hits) => {
                for h in hits.iter().take(5) {
                    let stars = if h.stars >= 0 {
                        format!("★{}", h.stars)
                    } else {
                        "-".into()
                    };
                    println!("  {:<48} {:>8}  {}", h.reference, stars, h.url);
                }
                println!("  ({} hits)", hits.len());
            }
            Err(e) => println!("FAILED: {e:#}"),
        }
    }
}
