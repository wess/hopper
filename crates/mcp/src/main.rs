//! `hoppermcp` — the stdio MCP server exposing Docker tools to AI clients.
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
    mcp::serve::run(host).await
}
