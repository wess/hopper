//! The stdio dispatch loop.

use crate::protocol::{self, codes};
use host::Host;
use serde_json::{json, Value};
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

/// Route one request to its handler.
pub async fn dispatch(host: &Arc<Host>, method: &str, params: &Value) -> Result<Value, (i32, String)> {
    match method {
        "initialize" => Ok(protocol::initialize_result(
            "hopper",
            env!("CARGO_PKG_VERSION"),
        )),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list()),
        "tools/call" => {
            let name = params
                .get("name")
                .and_then(|v| v.as_str())
                .ok_or((codes::INVALID_REQUEST, "tools/call needs a `name`.".to_string()))?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(crate::tools::call(host, name, &args).await)
        }
        other => Err((
            codes::METHOD_NOT_FOUND,
            format!("Unknown method: {other}"),
        )),
    }
}

fn tools_list() -> Value {
    crate::tools::catalogue()
}

/// Serve MCP over stdio until the client closes the stream.
pub async fn run(host: Arc<Host>) -> anyhow::Result<()> {
    let stdin = BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();
    let mut stdout = tokio::io::stdout();

    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let request = match protocol::parse(&line) {
            Ok(request) => request,
            Err(response) => {
                write(&mut stdout, &response).await?;
                continue;
            }
        };

        // Notifications are one-way; answering one is a protocol violation.
        if request.is_notification() {
            continue;
        }
        let id = request.id.clone().unwrap_or(Value::Null);

        let response = match dispatch(&host, &request.method, &request.params).await {
            Ok(result) => protocol::ok(id, result),
            Err((code, message)) => protocol::err(id, code, message),
        };
        write(&mut stdout, &response).await?;
    }
    Ok(())
}

async fn write(
    out: &mut tokio::io::Stdout,
    response: &protocol::Response,
) -> anyhow::Result<()> {
    let mut line = serde_json::to_string(response)?;
    line.push('\n');
    out.write_all(line.as_bytes()).await?;
    out.flush().await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use docker::{Client, Endpoint};

    fn host() -> Arc<Host> {
        Host::new(Client::new(Endpoint::Unix {
            path: "/nonexistent-hopper.sock".into(),
        }))
    }

    #[tokio::test]
    async fn initialize_reports_the_protocol_version() {
        let result = dispatch(&host(), "initialize", &json!({})).await.unwrap();
        assert_eq!(result["protocolVersion"], protocol::PROTOCOL_VERSION);
    }

    #[tokio::test]
    async fn tools_list_returns_the_catalogue() {
        let result = dispatch(&host(), "tools/list", &json!({})).await.unwrap();
        assert!(!result["tools"].as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn an_unknown_method_is_a_protocol_error() {
        let (code, _) = dispatch(&host(), "nope", &json!({})).await.unwrap_err();
        assert_eq!(code, codes::METHOD_NOT_FOUND);
    }

    #[tokio::test]
    async fn a_tool_call_without_a_name_is_rejected() {
        let (code, _) = dispatch(&host(), "tools/call", &json!({})).await.unwrap_err();
        assert_eq!(code, codes::INVALID_REQUEST);
    }

    #[tokio::test]
    async fn an_unknown_tool_fails_inside_the_result_not_as_a_protocol_error() {
        // The call was well-formed; the tool just does not exist. Reporting a
        // protocol error would make clients retry.
        let result = dispatch(
            &host(),
            "tools/call",
            &json!({ "name": "docker.nope", "arguments": {} }),
        )
        .await
        .unwrap();
        assert_eq!(result["isError"], true);
    }

    #[tokio::test]
    async fn a_tool_reports_an_unreachable_daemon_rather_than_hanging() {
        let result = dispatch(
            &host(),
            "tools/call",
            &json!({ "name": "docker.list_containers", "arguments": {} }),
        )
        .await
        .unwrap();
        assert_eq!(result["isError"], true);
    }
}
