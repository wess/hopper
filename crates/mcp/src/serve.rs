//! The stdio dispatch loop.

use crate::protocol::{self, codes};
use futures::{
    future::{AbortHandle, Abortable},
    stream::FuturesUnordered,
    StreamExt,
};
use host::Host;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use tokio::io::AsyncWriteExt;
use tokio_util::codec::{FramedRead, LinesCodec};

const MAX_FRAME_BYTES: usize = 4 * 1024 * 1024;

/// Route one request to its handler.
pub async fn dispatch(
    host: &Arc<Host>,
    method: &str,
    params: &Value,
) -> Result<Value, (i32, String)> {
    match method {
        "initialize" => Ok(protocol::initialize_result(
            "hopper",
            env!("CARGO_PKG_VERSION"),
        )),
        "ping" => Ok(json!({})),
        "tools/list" => Ok(tools_list()),
        "tools/call" => {
            let name = params.get("name").and_then(|v| v.as_str()).ok_or((
                codes::INVALID_REQUEST,
                "tools/call needs a `name`.".to_string(),
            ))?;
            let args = params.get("arguments").cloned().unwrap_or(json!({}));
            Ok(crate::tools::call(host, name, &args).await)
        }
        other => Err((codes::METHOD_NOT_FOUND, format!("Unknown method: {other}"))),
    }
}

fn tools_list() -> Value {
    crate::tools::catalogue()
}

/// Serve MCP over stdio until the client closes the stream.
pub async fn run(host: Arc<Host>) -> anyhow::Result<()> {
    let mut lines = FramedRead::new(
        tokio::io::stdin(),
        LinesCodec::new_with_max_length(MAX_FRAME_BYTES),
    );
    let mut stdout = tokio::io::stdout();

    type Pending = Pin<Box<dyn Future<Output = (String, protocol::Response)> + Send>>;
    let mut pending = FuturesUnordered::<Pending>::new();
    let mut cancellation = HashMap::<String, AbortHandle>::new();
    loop {
        tokio::select! {
            line=lines.next()=> {
                let Some(line)=line else {break;};
                let line=line?;
                if line.trim().is_empty() {continue;}
                let request=match protocol::parse(&line) {
                    Ok(request)=>request,
                    Err(response)=>{write(&mut stdout,&response).await?;continue;}
                };
                if request.is_notification() {
                    if request.method=="notifications/cancelled" {
                        if let Some(id)=request.params.get("requestId") {
                            if let Some(handle)=cancellation.get(&serde_json::to_string(id)?) {handle.abort();}
                        }
                    }
                    continue;
                }
                let id=request.id.clone().unwrap_or(Value::Null);
                let key=serde_json::to_string(&id)?;
                if cancellation.contains_key(&key) || pending.len()>=16 {
                    let response=protocol::err(id,codes::INVALID_REQUEST,"Duplicate request id or too many concurrent requests");
                    write(&mut stdout,&response).await?;
                    continue;
                }
                let (abort,registration)=AbortHandle::new_pair();
                cancellation.insert(key.clone(),abort);
                let host=Arc::clone(&host);
                pending.push(Box::pin(async move {
                    let result=Abortable::new(dispatch(&host,&request.method,&request.params),registration).await;
                    let response=match result {
                        Ok(Ok(result))=>protocol::ok(id,result),
                        Ok(Err((code,message)))=>protocol::err(id,code,message),
                        Err(_)=>protocol::err(id,-32800,"Request cancelled. Check the VM status before retrying a lifecycle operation."),
                    };
                    (key,response)
                }));
            }
            result=pending.next(),if !pending.is_empty()=> {
                if let Some((key,response))=result {
                    cancellation.remove(&key);
                    write(&mut stdout,&response).await?;
                }
            }
        }
    }
    Ok(())
}

async fn write(out: &mut tokio::io::Stdout, response: &protocol::Response) -> anyhow::Result<()> {
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
        let (code, _) = dispatch(&host(), "tools/call", &json!({}))
            .await
            .unwrap_err();
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
