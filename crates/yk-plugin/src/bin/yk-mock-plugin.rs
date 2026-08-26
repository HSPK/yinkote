//! Minimal reference plugin used by the integration tests.
//!
//! It is also the smallest possible worked example of the protocol: read a
//! JSON line, write a JSON line.

use std::io::{BufRead, Write};

fn main() {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    let mut lines = stdin.lock().lines();

    while let Some(Ok(line)) = lines.next() {
        if line.trim().is_empty() {
            continue;
        }
        let msg: serde_json::Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let id = msg.get("id").and_then(|v| v.as_u64()).unwrap_or(0);
        let method = msg.get("method").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let params = msg.get("params").cloned().unwrap_or(serde_json::Value::Null);

        let result = match method.as_str() {
            "initialize" => serde_json::json!({
                "contributions": {
                    "metadataSources": [
                        { "id": "mock", "label": "Mock Source", "supports": ["query", "doi"] }
                    ],
                    "importers": [
                        { "id": "mockfmt", "label": "Mock Format", "extensions": ["mock"] }
                    ]
                }
            }),
            "hook" => serde_json::json!({ "seen": params.get("name") }),
            "echo" => params,
            "boom" => {
                reply_error(&mut stdout, id, -32000, "intentional failure");
                continue;
            }
            "slow" => {
                std::thread::sleep(std::time::Duration::from_secs(30));
                serde_json::Value::Null
            }
            "crash" => std::process::exit(3),
            "callhost" => {
                // Demonstrates the reverse direction of the protocol.
                let req = serde_json::json!({
                    "jsonrpc": "2.0", "id": 9001,
                    "method": params.get("method").and_then(|v| v.as_str()).unwrap_or("host.log"),
                    "params": params.get("params").cloned().unwrap_or(serde_json::Value::Null),
                });
                writeln!(stdout, "{req}").ok();
                stdout.flush().ok();
                match lines.next() {
                    Some(Ok(resp)) => serde_json::from_str::<serde_json::Value>(&resp)
                        .unwrap_or(serde_json::Value::Null),
                    _ => serde_json::Value::Null,
                }
            }
            "shutdown" => {
                reply_ok(&mut stdout, id, serde_json::Value::Null);
                return;
            }
            _ => {
                reply_error(&mut stdout, id, -32601, format!("unknown method '{method}'"));
                continue;
            }
        };
        reply_ok(&mut stdout, id, result);
    }
}

fn reply_ok(out: &mut impl Write, id: u64, result: serde_json::Value) {
    let msg = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
    writeln!(out, "{msg}").ok();
    out.flush().ok();
}

fn reply_error(out: &mut impl Write, id: u64, code: i64, message: impl Into<String>) {
    let msg = serde_json::json!({
        "jsonrpc": "2.0", "id": id,
        "error": { "code": code, "message": message.into() }
    });
    writeln!(out, "{msg}").ok();
    out.flush().ok();
}
