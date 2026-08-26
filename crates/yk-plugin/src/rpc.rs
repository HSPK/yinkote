//! Bidirectional JSON-RPC 2.0 over line-delimited JSON.
//!
//! Both sides may originate requests on the same stream: the host calls into
//! the plugin, and the plugin calls back into the host API. Messages are
//! newline-delimited so any language can implement a plugin with a `readline`
//! loop and `print`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const JSONRPC: &str = "2.0";

#[derive(Debug, Clone, Serialize)]
pub struct Request {
    pub jsonrpc: &'static str,
    pub id: u64,
    pub method: String,
    pub params: Value,
}

impl Request {
    pub fn new(id: u64, method: impl Into<String>, params: Value) -> Self {
        Self { jsonrpc: JSONRPC, id, method: method.into(), params }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Response {
    pub jsonrpc: &'static str,
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<RpcError>,
}

impl Response {
    pub fn ok(id: u64, result: Value) -> Self {
        Self { jsonrpc: JSONRPC, id, result: Some(result), error: None }
    }
    pub fn err(id: u64, code: i64, message: impl Into<String>) -> Self {
        Self {
            jsonrpc: JSONRPC,
            id,
            result: None,
            error: Some(RpcError { code, message: message.into(), data: None }),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpcError {
    pub code: i64,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

/// Standard JSON-RPC error codes plus our permission code.
pub mod codes {
    pub const PARSE_ERROR: i64 = -32700;
    pub const INVALID_REQUEST: i64 = -32600;
    pub const METHOD_NOT_FOUND: i64 = -32601;
    pub const INVALID_PARAMS: i64 = -32602;
    pub const INTERNAL_ERROR: i64 = -32603;
    pub const PERMISSION_DENIED: i64 = -32000;
}

/// Anything that can arrive on the wire from the peer.
#[derive(Debug, Deserialize)]
pub struct Incoming {
    #[serde(default)]
    pub id: Option<u64>,
    #[serde(default)]
    pub method: Option<String>,
    #[serde(default)]
    pub params: Option<Value>,
    #[serde(default)]
    pub result: Option<Value>,
    #[serde(default)]
    pub error: Option<RpcError>,
}

impl Incoming {
    /// A message with a `method` is a request *to us*.
    pub fn is_request(&self) -> bool {
        self.method.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_serialises_canonically() {
        let r = Request::new(7, "hook", serde_json::json!({"name": "startup"}));
        let s = serde_json::to_string(&r).unwrap();
        assert!(s.contains(r#""jsonrpc":"2.0""#));
        assert!(s.contains(r#""id":7"#));
        assert!(s.contains(r#""method":"hook""#));
    }

    #[test]
    fn response_omits_absent_fields() {
        let s = serde_json::to_string(&Response::ok(1, Value::Null)).unwrap();
        assert!(!s.contains("error"));
        let s = serde_json::to_string(&Response::err(1, -1, "x")).unwrap();
        assert!(!s.contains("result"));
    }

    #[test]
    fn classifies_incoming_messages() {
        let req: Incoming =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"method":"host.log"}"#).unwrap();
        assert!(req.is_request());

        let resp: Incoming =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"result":{"ok":true}}"#).unwrap();
        assert!(!resp.is_request());
        assert!(resp.result.is_some());
    }

    #[test]
    fn tolerates_unknown_fields() {
        let m: Incoming =
            serde_json::from_str(r#"{"jsonrpc":"2.0","id":1,"result":1,"extra":"x"}"#).unwrap();
        assert_eq!(m.id, Some(1));
    }
}
