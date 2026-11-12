//! Shared bounded MCP framing and lifecycle for headless and live hosts.
#[cfg(unix)]
pub mod ipc;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
#[derive(Default)]
pub struct Protocol {
    initialized: bool,
    ready: bool,
}
impl Protocol {
    pub fn handle(
        &mut self,
        message: Value,
        catalog: &Value,
        mut call: impl FnMut(&str, &Value) -> std::result::Result<Value, String>,
    ) -> Option<Value> {
        let id = message.get("id").cloned();
        let method = message["method"].as_str().unwrap_or("");
        if message["jsonrpc"] != "2.0"
            || method.is_empty()
            || id
                .as_ref()
                .is_some_and(|id| !(id.is_string() || id.is_number()))
        {
            return Some(
                json!({"jsonrpc":"2.0","id":id.unwrap_or(Value::Null),"error":{"code":-32600,"message":"invalid request"}}),
            );
        }
        if id.is_none() {
            if method == "notifications/initialized" && self.initialized {
                self.ready = true;
            }
            return None;
        }
        let response = match method {
            "initialize" if !self.initialized => {
                self.initialized = true;
                json!({"result":{"protocolVersion":"2025-11-25","capabilities":{"tools":{}},"serverInfo":{"name":"veylvik","version":env!("CARGO_PKG_VERSION")}}})
            }
            "ping" => json!({"result":{}}),
            _ if !self.ready => {
                json!({"error":{"code":-32002,"message":"initialize and send notifications/initialized first"}})
            }
            "tools/list" => json!({"result":catalog}),
            "tools/call" => {
                let params = &message["params"];
                let name = params["name"].as_str().unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                match call(name, &args) {
                    Ok(value) => {
                        json!({"result":{"content":[{"type":"text","text":value.to_string()}],"structuredContent":value,"isError":false}})
                    }
                    Err(e) => {
                        json!({"result":{"content":[{"type":"text","text":e.to_string()}],"isError":true}})
                    }
                }
            }
            _ => json!({"error":{"code":-32601,"message":"method not found"}}),
        };
        let mut response = response;
        response["jsonrpc"] = json!("2.0");
        response["id"] = id.unwrap();
        Some(response)
    }
}
pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    catalog: &Value,
    mut call: impl FnMut(&str, &Value) -> std::result::Result<Value, String>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    let mut protocol = Protocol::default();
    loop {
        let mut bytes = Vec::new();
        // Bound framing memory even when a client omits its newline.
        let count = std::io::Read::take(&mut input, MAX_MESSAGE_BYTES as u64 + 1)
            .read_until(b'\n', &mut bytes)?;
        if count == 0 {
            break;
        }
        if count > MAX_MESSAGE_BYTES {
            return Err("MCP message exceeds 4 MiB".into());
        }
        let response = match serde_json::from_slice(&bytes) {
            Ok(value) => protocol.handle(value, catalog, &mut call),
            Err(_) => Some(
                json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":"invalid JSON"}}),
            ),
        };
        if let Some(response) = response {
            serde_json::to_writer(&mut output, &response)?;
            writeln!(output)?;
            output.flush()?;
        }
    }
    Ok(())
}
