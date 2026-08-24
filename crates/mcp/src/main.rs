//! `hoppermcp` — the stdio MCP server exposing container tools to AI clients.
//!
//! Logs go to stderr: stdout is the protocol transport, and a stray line there
//! corrupts every frame after it.

use host::Host;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "warn".into()),
        )
        .init();

    let host = Host::from_env();
    // Pick an engine the way the app does. Without this the server only ever
    // holds the default Docker socket, so on a Mac running Apple Containers
    // every tool would answer "no engine" — Docker is not a requirement here.
    let status = host.select_engine().await;
    tracing::info!("engine: {} ({})", status.provider, status.message);

    mcp::serve::run(host).await
}
