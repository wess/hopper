//! JSON-RPC framing for the Model Context Protocol over stdio.
//!
//! Transport only: the caller supplies the tool list and the handler. Keeping
//! it free of Docker knowledge means the framing can be tested on its own,
//! which matters because a malformed frame silently breaks every AI client
//! that connects.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// The protocol revision this server implements.
pub const PROTOCOL_VERSION: &str = "2024-11-05";

#[derive(Debug, Clone, Deserialize)]
pub struct Request {
    #[serde(default)]
    pub jsonrpc: String,
    /// Absent for notifications, which must not be answered.
    #[serde(default)]
    pub id: Option<Value>,
    pub method: String,
    #[serde(default)]
    pub params: Value,
}

impl Request {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorObject>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ErrorObject {
    pub code: i32,
    pub message: String,
}

/// JSON-RPC's reserved codes, the few we actually emit.
pub mod codes {
    pub const PARSE_ERROR: i32 = -32700;
    pub const INVALID_REQUEST: i32 = -32600;
    pub const METHOD_NOT_FOUND: i32 = -32601;
    pub const INTERNAL_ERROR: i32 = -32603;
}

pub fn ok(id: Value, result: Value) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: Some(result),
        error: None,
    }
}

pub fn err(id: Value, code: i32, message: impl Into<String>) -> Response {
    Response {
        jsonrpc: "2.0",
        id,
        result: None,
        error: Some(ErrorObject {
            code,
            message: message.into(),
        }),
    }
}

/// Parse one line into a request.
pub fn parse(line: &str) -> Result<Request, Response> {
    serde_json::from_str::<Request>(line).map_err(|e| {
        // An unparseable frame has no id to correlate with, so the spec says
        // to answer with a null id rather than staying silent.
        err(Value::Null, codes::PARSE_ERROR, format!("Invalid JSON: {e}"))
    })
}

/// The `initialize` result.
pub fn initialize_result(name: &str, version: &str) -> Value {
    json!({
        "protocolVersion": PROTOCOL_VERSION,
        "capabilities": { "tools": { "listChanged": false } },
        "serverInfo": { "name": name, "version": version }
    })
}

/// Wrap text as an MCP tool result.
pub fn text_result(text: impl Into<String>) -> Value {
    json!({ "content": [{ "type": "text", "text": text.into() }] })
}

/// Wrap an error as a tool result.
///
/// A failed tool call is reported *inside* the result with `isError`, not as a
/// JSON-RPC error — the latter means "the call was malformed", and conflating
/// the two makes clients retry things that will never succeed.
pub fn error_result(message: impl Into<String>) -> Value {
    json!({
        "content": [{ "type": "text", "text": message.into() }],
        "isError": true
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_request_parses() {
        let req = parse(r#"{"jsonrpc":"2.0","id":1,"method":"tools/list"}"#).unwrap();
        assert_eq!(req.method, "tools/list");
        assert!(!req.is_notification());
    }

    #[test]
    fn a_notification_has_no_id_and_must_not_be_answered() {
        let req = parse(r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#).unwrap();
        assert!(req.is_notification());
    }

    #[test]
    fn unparseable_input_answers_with_a_null_id_rather_than_silence() {
        let response = parse("{not json").unwrap_err();
        assert_eq!(response.id, Value::Null);
        assert_eq!(response.error.unwrap().code, codes::PARSE_ERROR);
    }

    #[test]
    fn a_successful_response_carries_no_error_key() {
        let raw = serde_json::to_string(&ok(json!(1), json!({"x": 1}))).unwrap();
        assert!(raw.contains(r#""result""#));
        assert!(
            !raw.contains(r#""error""#),
            "a result and an error must never appear together"
        );
    }

    #[test]
    fn an_error_response_carries_no_result_key() {
        let raw = serde_json::to_string(&err(json!(1), codes::METHOD_NOT_FOUND, "nope")).unwrap();
        assert!(raw.contains(r#""error""#));
        assert!(!raw.contains(r#""result""#));
    }

    #[test]
    fn responses_serialize_to_a_single_line() {
        // The transport is newline-delimited; an embedded newline would split
        // one response into two frames.
        let raw = serde_json::to_string(&ok(json!(1), json!({"text": "a\nb"}))).unwrap();
        assert!(!raw.contains('\n'));
    }

    #[test]
    fn initialize_advertises_the_protocol_version_and_tools() {
        let result = initialize_result("hopper", "0.6.0");
        assert_eq!(result["protocolVersion"], PROTOCOL_VERSION);
        assert!(result["capabilities"]["tools"].is_object());
        assert_eq!(result["serverInfo"]["name"], "hopper");
    }

    #[test]
    fn a_failed_tool_call_is_flagged_in_the_result_not_as_a_protocol_error() {
        let result = error_result("no such container");
        assert_eq!(result["isError"], true);
        assert_eq!(result["content"][0]["text"], "no such container");
    }

    #[test]
    fn a_text_result_has_no_error_flag() {
        let result = text_result("fine");
        assert!(result.get("isError").is_none());
    }
}
