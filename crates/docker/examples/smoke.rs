//! Live smoke test against whatever daemon the environment points at.
//!
//! `cargo run -p docker --example smoke`
//!
//! Exercises the paths that only a real daemon can prove: the socket
//! handshake, version negotiation, a JSON list, an ndjson stream, and the
//! error path for a missing resource.

use docker::client::Req;
use docker::Client;

#[tokio::main]
async fn main() {
    let client = Client::from_env();
    println!("endpoint : {}", client.endpoint().describe());

    match client.ping().await {
        Ok(()) => println!("ping     : ok"),
        Err(e) => {
            println!("ping     : FAILED {e}");
            return;
        }
    }

    match client.negotiate().await {
        Ok(v) => println!("api      : negotiated v{v}"),
        Err(e) => println!("api      : FAILED {e}"),
    }

    let version: serde_json::Value = client
        .json(Req::get("/version").unversioned())
        .await
        .unwrap_or_default();
    println!(
        "daemon   : {} (api {})",
        version["Version"].as_str().unwrap_or("?"),
        version["ApiVersion"].as_str().unwrap_or("?")
    );

    let containers: Vec<serde_json::Value> = client
        .json(Req::get("/containers/json").flag("all", true))
        .await
        .unwrap_or_default();
    println!("containers: {}", containers.len());
    for c in containers.iter().take(3) {
        println!(
            "  - {} {}",
            c["Names"][0].as_str().unwrap_or("?"),
            c["State"].as_str().unwrap_or("?")
        );
    }

    let images: Vec<serde_json::Value> = client
        .json(Req::get("/images/json"))
        .await
        .unwrap_or_default();
    println!("images   : {}", images.len());

    // The error path: the daemon's own message must survive, not be replaced
    // by a generic failure string.
    match client
        .json::<serde_json::Value>(Req::get("/containers/hopper-does-not-exist/json"))
        .await
    {
        Ok(_) => println!("error    : UNEXPECTED success"),
        Err(e) => println!("error    : {} (status {:?})", e, e.status),
    }

    // An ndjson stream: read a couple of events with a short deadline, since
    // an idle daemon emits nothing.
    let mut seen = 0usize;
    let stream = client.ndjson::<serde_json::Value, _>(
        Req::get("/events").no_timeout(),
        |_| {
            seen += 1;
            seen < 2
        },
    );
    match tokio::time::timeout(std::time::Duration::from_millis(600), stream).await {
        Ok(Ok(())) => println!("events   : stream closed after {seen}"),
        Ok(Err(e)) => println!("events   : FAILED {e}"),
        Err(_) => println!("events   : open, idle (drop-cancel worked)"),
    }
}
