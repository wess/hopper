//! The Docker tools exposed to AI clients.

use host::Host;
use model::LogOptions;
use serde_json::{json, Value};
use std::sync::Arc;

/// The tool catalogue, as `tools/list` returns it.
pub fn catalogue() -> Value {
    json!({
        "tools": [
            {
                "name": "docker.list_containers",
                "description": "List containers. Set `all` to include stopped ones.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "all": { "type": "boolean", "description": "Include stopped containers." }
                    }
                }
            },
            {
                "name": "docker.list_images",
                "description": "List images.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "docker.list_volumes",
                "description": "List volumes.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "docker.list_networks",
                "description": "List networks.",
                "inputSchema": { "type": "object", "properties": {} }
            },
            {
                "name": "docker.logs",
                "description": "Fetch the most recent log lines from a container (one-shot, not following).",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "container": { "type": "string", "description": "Container id or name." },
                        "tail": { "type": "integer", "description": "How many lines (default 200)." }
                    },
                    "required": ["container"]
                }
            },
            {
                "name": "docker.start",
                "description": "Start a container.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "container": { "type": "string" } },
                    "required": ["container"]
                }
            },
            {
                "name": "docker.stop",
                "description": "Stop a container.",
                "inputSchema": {
                    "type": "object",
                    "properties": { "container": { "type": "string" } },
                    "required": ["container"]
                }
            },
            {
                "name": "docker.system_info",
                "description": "Engine version, resource counts, and disk usage.",
                "inputSchema": { "type": "object", "properties": {} }
            }
        ]
    })
}

fn arg_str<'a>(args: &'a Value, key: &str) -> Option<&'a str> {
    args.get(key).and_then(|v| v.as_str()).filter(|s| !s.is_empty())
}

/// Run one tool call, returning its MCP result payload.
pub async fn call(host: &Arc<Host>, name: &str, args: &Value) -> Value {
    use crate::protocol::{error_result, text_result};

    // A tool that acts on a container is useless without one, and the daemon's
    // error for an empty id is unhelpful, so it is caught here.
    let need_container = || -> Result<String, Value> {
        arg_str(args, "container")
            .map(str::to_string)
            .ok_or_else(|| error_result("This tool needs a `container` id or name."))
    };

    match name {
        "docker.list_containers" => {
            let all = args.get("all").and_then(|v| v.as_bool()).unwrap_or(false);
            match host.containers(all).await {
                Ok(list) => {
                    let rows: Vec<Value> = list
                        .iter()
                        .map(|c| {
                            json!({
                                "id": c.short_id(),
                                "name": c.name,
                                "image": c.image,
                                "state": c.state.as_str(),
                                "health": c.health.as_str(),
                                "status": c.status,
                                "composeProject": c.compose_project,
                            })
                        })
                        .collect();
                    text_result(serde_json::to_string_pretty(&rows).unwrap_or_default())
                }
                Err(e) => error_result(e.message),
            }
        }
        "docker.list_images" => match host.images(false).await {
            Ok(list) => {
                let rows: Vec<Value> = list
                    .iter()
                    .map(|i| json!({ "id": i.short_id(), "tags": i.repo_tags, "size": i.size }))
                    .collect();
                text_result(serde_json::to_string_pretty(&rows).unwrap_or_default())
            }
            Err(e) => error_result(e.message),
        },
        "docker.list_volumes" => match host.volumes().await {
            Ok(list) => {
                let rows: Vec<Value> = list
                    .iter()
                    .map(|v| json!({ "name": v.name, "driver": v.driver, "inUse": v.in_use }))
                    .collect();
                text_result(serde_json::to_string_pretty(&rows).unwrap_or_default())
            }
            Err(e) => error_result(e.message),
        },
        "docker.list_networks" => match host.networks().await {
            Ok(list) => {
                let rows: Vec<Value> = list
                    .iter()
                    .map(|n| json!({ "name": n.name, "driver": n.driver, "containers": n.containers }))
                    .collect();
                text_result(serde_json::to_string_pretty(&rows).unwrap_or_default())
            }
            Err(e) => error_result(e.message),
        },
        "docker.logs" => {
            let id = match need_container() {
                Ok(id) => id,
                Err(result) => return result,
            };
            let tail = args
                .get("tail")
                .and_then(|v| v.as_u64())
                .unwrap_or(200)
                .min(5_000) as u32;
            let opts = LogOptions {
                tail,
                follow: false,
                ..Default::default()
            };
            let mut out = String::new();
            match host
                .stream_logs("mcp", &id, &opts, |line| {
                    out.push_str(&line.text);
                    out.push('\n');
                    true
                })
                .await
            {
                Ok(()) if out.is_empty() => text_result("(no output)"),
                Ok(()) => text_result(out),
                Err(e) => error_result(e.message),
            }
        }
        "docker.start" => {
            let id = match need_container() {
                Ok(id) => id,
                Err(result) => return result,
            };
            match host.container_start(&id).await {
                Ok(()) => text_result(format!("Started {id}.")),
                Err(e) => error_result(e.message),
            }
        }
        "docker.stop" => {
            let id = match need_container() {
                Ok(id) => id,
                Err(result) => return result,
            };
            match host.container_stop(&id).await {
                Ok(()) => text_result(format!("Stopped {id}.")),
                Err(e) => error_result(e.message),
            }
        }
        "docker.system_info" => {
            let info = host.info().await;
            let usage = host.disk_usage().await;
            match info {
                Ok(i) => {
                    let payload = json!({
                        "name": i.name,
                        "serverVersion": i.server_version,
                        "containers": i.containers,
                        "containersRunning": i.containers_running,
                        "images": i.images,
                        "ncpu": i.ncpu,
                        "memTotal": i.mem_total,
                        "diskUsage": usage.ok().map(|u| json!({
                            "total": u.total_size(),
                            "reclaimable": u.total_reclaimable(),
                        })),
                    });
                    text_result(serde_json::to_string_pretty(&payload).unwrap_or_default())
                }
                Err(e) => error_result(e.message),
            }
        }
        other => error_result(format!("Unknown tool: {other}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tool_names() -> Vec<String> {
        catalogue()["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap().to_string())
            .collect()
    }

    #[test]
    fn every_tool_declares_a_name_description_and_schema() {
        for tool in catalogue()["tools"].as_array().unwrap() {
            assert!(tool["name"].as_str().is_some_and(|n| !n.is_empty()));
            assert!(tool["description"].as_str().is_some_and(|d| !d.is_empty()));
            assert_eq!(
                tool["inputSchema"]["type"], "object",
                "clients reject a tool whose schema is not an object"
            );
        }
    }

    #[test]
    fn the_catalogue_covers_the_core_docker_surface() {
        let names = tool_names();
        for expected in [
            "docker.list_containers",
            "docker.list_images",
            "docker.logs",
            "docker.start",
            "docker.stop",
            "docker.system_info",
        ] {
            assert!(names.contains(&expected.to_string()), "missing {expected}");
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let mut names = tool_names();
        let before = names.len();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), before, "a duplicate name shadows a tool");
    }

    #[test]
    fn container_tools_require_a_container_argument() {
        for tool in catalogue()["tools"].as_array().unwrap() {
            let name = tool["name"].as_str().unwrap();
            if matches!(name, "docker.logs" | "docker.start" | "docker.stop") {
                let required = tool["inputSchema"]["required"].as_array().unwrap();
                assert!(required.iter().any(|r| r == "container"), "{name}");
            }
        }
    }

    #[test]
    fn argument_extraction_treats_blank_as_absent() {
        let args = json!({ "container": "" });
        assert!(arg_str(&args, "container").is_none());
        let args = json!({ "container": "web" });
        assert_eq!(arg_str(&args, "container"), Some("web"));
    }
}
