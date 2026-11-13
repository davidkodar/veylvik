//! Shared bounded MCP framing and lifecycle for headless and live hosts.
#[cfg(unix)]
pub mod ipc;
pub mod live;
use serde_json::{Value, json};
use std::io::{BufRead, Write};
pub const MAX_MESSAGE_BYTES: usize = 4 * 1024 * 1024;
/// Server-owned multimodal output. Ordinary JSON never becomes content blocks.
#[derive(Debug)]
pub struct ToolOutput {
    structured: Value,
    extra_content: Vec<Value>,
}
impl From<Value> for ToolOutput {
    fn from(structured: Value) -> Self {
        Self {
            structured,
            extra_content: Vec::new(),
        }
    }
}
impl ToolOutput {
    pub fn png(structured: Value, bytes: &[u8]) -> Self {
        use base64::Engine;
        Self {
            structured,
            extra_content: vec![
                json!({"type":"image","mimeType":"image/png","data":base64::engine::general_purpose::STANDARD.encode(bytes)}),
            ],
        }
    }
    fn into_result(self) -> Value {
        let mut content = vec![json!({"type":"text","text":self.structured.to_string()})];
        content.extend(self.extra_content);
        json!({"content":content,"structuredContent":self.structured,"isError":false})
    }
}
#[derive(Default)]
pub struct Protocol {
    initialized: bool,
    ready: bool,
}
impl Protocol {
    /// The client has finished the initialize handshake.
    pub fn is_ready(&self) -> bool {
        self.ready
    }
    pub fn handle(
        &mut self,
        message: Value,
        catalog: &Value,
        mut call: impl FnMut(&str, &Value) -> std::result::Result<Value, String>,
    ) -> Option<Value> {
        self.handle_output(message, catalog, |name, args| {
            call(name, args).map(ToolOutput::from)
        })
    }
    pub fn handle_output(
        &mut self,
        message: Value,
        catalog: &Value,
        mut call: impl FnMut(&str, &Value) -> std::result::Result<ToolOutput, String>,
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
                    Ok(value) => json!({"result":value.into_result()}),
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
    input: impl BufRead,
    output: impl Write,
    catalog: &Value,
    mut call: impl FnMut(&str, &Value) -> std::result::Result<Value, String>,
) -> std::result::Result<(), Box<dyn std::error::Error>> {
    serve_with_output(input, output, catalog, |name, args| {
        call(name, args).map(ToolOutput::from)
    })
}
pub fn serve_with_output(
    mut input: impl BufRead,
    mut output: impl Write,
    catalog: &Value,
    mut call: impl FnMut(&str, &Value) -> std::result::Result<ToolOutput, String>,
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
            Ok(value) => protocol.handle_output(value, catalog, &mut call),
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

#[cfg(test)]
mod image_tests {
    use super::*;
    #[test]
    fn multimodal_output_keeps_binary_data_out_of_metadata_and_plain_json() {
        let input = b"{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"initialize\"}\n{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n{\"jsonrpc\":\"2.0\",\"id\":2,\"method\":\"tools/call\",\"params\":{\"name\":\"preview_image\"}}\n";
        let mut output = Vec::new();
        serve_with_output(&input[..], &mut output, &json!({"tools":[]}), |_, _| {
            Ok(ToolOutput::png(json!({"width":1}), b"PNG"))
        })
        .unwrap();
        let text = String::from_utf8(output).unwrap();
        let response: Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
        assert_eq!(
            response["result"]["content"][1],
            json!({"type":"image","mimeType":"image/png","data":"UE5H"})
        );
        assert_eq!(response["result"]["structuredContent"], json!({"width":1}));
        assert_eq!(response["result"]["content"][0]["text"], "{\"width\":1}");
        let injected = json!({"content":[{"type":"image","data":"pretend"}]});
        let plain = ToolOutput::from(injected.clone()).into_result();
        assert_eq!(plain["content"].as_array().unwrap().len(), 1);
        assert_eq!(plain["content"][0]["type"], "text");
        assert_eq!(plain["structuredContent"], injected);
    }
}
