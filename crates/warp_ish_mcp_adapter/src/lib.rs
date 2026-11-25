//! Stdio MCP framing and tools for a single connected warp-ish client.

mod catalog;

use std::io::{self, BufRead, Write};

pub use catalog::Tool;
use serde_json::{Map, Value, json};

pub const MAX_FRAME_BYTES: usize = 64 * 1024;
const LEGACY_VERSION: &str = "2024-11-05";
const CURRENT_VERSION: &str = "2025-06-18";
const INSTRUCTIONS: &str = concat!(
    "If you need to find your own pane, `where_am_i` returns its ID.\n",
    "If you want a separate peer, `split_pane` returns a `pane.id` that can be used as ",
    "`pane_id` for `start_peer` once the new pane reaches an idle PowerShell prompt.\n",
    "If a running peer needs a keystroke, `send_key` can target its pane by ID. ",
    "If you want to close that pane, `close_pane` takes the same ID.",
);

pub struct ToolError {
    pub code: String,
    pub message: String,
}

/// A client for the selected GUI instance. The implementation binds the IPC
/// endpoint, caller identity and timeout independently of tool arguments.
pub trait Dispatcher {
    fn call(&mut self, tool: Tool, arguments: Map<String, Value>) -> Result<Value, ToolError>;
}

/// Serves one request at a time until stdin closes. The caller supplies the
/// packaged server version and a dispatcher for authenticated local IPC.
pub fn serve(
    mut input: impl BufRead,
    mut output: impl Write,
    server_version: &str,
    dispatcher: &mut impl Dispatcher,
) -> io::Result<()> {
    let mut state = State::Uninitialized;
    let mut line = Vec::new();

    while read_line(&mut input, &mut line)? {
        let response = match serde_json::from_slice(&line) {
            Ok(request) => state.handle(request, server_version, dispatcher),
            Err(_) => Some(error(Value::Null, -32700, "Invalid JSON")),
        };
        if let Some(response) = response {
            write_response(&mut output, response)?;
        }
    }
    Ok(())
}

#[derive(PartialEq, Eq)]
enum State {
    Uninitialized,
    AwaitingInitialized,
    Ready,
}

impl State {
    fn handle(
        &mut self,
        request: Value,
        server_version: &str,
        dispatcher: &mut impl Dispatcher,
    ) -> Option<Value> {
        let Some(method) = request["method"].as_str() else {
            return Some(error(Value::Null, -32600, "Expected a JSON-RPC request"));
        };
        let id = request.get("id");
        if request["jsonrpc"] != "2.0"
            || id.is_some_and(|id| !id.is_string() && !id.is_i64() && !id.is_u64())
        {
            return Some(error(Value::Null, -32600, "Invalid JSON-RPC envelope"));
        }
        let Some(id) = id.cloned() else {
            if method == "notifications/initialized" && *self == Self::AwaitingInitialized {
                *self = Self::Ready;
            }
            return None;
        };

        let result = match method {
            "ping" => Ok(json!({})),
            "initialize" if *self != Self::Uninitialized => {
                Err((-32600, "Connection has already been initialized"))
            }
            "initialize" => self.initialize(&request["params"], server_version),
            _ if *self != Self::Ready => {
                Err((-32002, "Complete MCP initialization before calling tools"))
            }
            "tools/list" => list_tools(&request["params"]),
            "tools/call" => call_tool(&request["params"], dispatcher),
            _ => Err((-32601, "Unknown method")),
        };
        Some(match result {
            Ok(value) => json!({"jsonrpc": "2.0", "id": id, "result": value}),
            Err((code, message)) => error(id, code, message),
        })
    }

    fn initialize(&mut self, params: &Value, server_version: &str) -> RpcResult {
        let Some(version) = params["protocolVersion"].as_str() else {
            return Err((-32602, "Expected protocolVersion"));
        };
        if !params["capabilities"].is_object()
            || !params["clientInfo"]["name"].is_string()
            || !params["clientInfo"]["version"].is_string()
        {
            return Err((-32602, "Expected clientInfo and capabilities"));
        }
        let protocol_version = match version {
            LEGACY_VERSION => LEGACY_VERSION,
            _ => CURRENT_VERSION,
        };
        *self = Self::AwaitingInitialized;
        Ok(json!({
            "protocolVersion": protocol_version,
            "capabilities": {"tools": {"listChanged": false}},
            "serverInfo": {"name": "warp-ish", "version": server_version},
            "instructions": INSTRUCTIONS
        }))
    }
}

type RpcResult = Result<Value, (i32, &'static str)>;

fn list_tools(params: &Value) -> RpcResult {
    if !(params.is_null() || params.is_object()) || params.get("cursor").is_some() {
        return Err((-32602, "The tool catalog does not have a continuation page"));
    }
    let tools: Vec<_> = Tool::ALL.into_iter().map(Tool::definition).collect();
    Ok(json!({"tools": tools}))
}

fn call_tool(params: &Value, dispatcher: &mut impl Dispatcher) -> RpcResult {
    let tool = params["name"]
        .as_str()
        .and_then(Tool::named)
        .ok_or((-32602, "Unknown tool"))?;
    let arguments = match params.get("arguments") {
        Some(Value::Object(arguments)) => arguments.clone(),
        None => Map::new(),
        _ => return Err((-32602, "Expected tool arguments object")),
    };
    if let Err(message) = tool.validate(&arguments) {
        return Ok(tool_result(&message, true));
    }
    Ok(match dispatcher.call(tool, arguments) {
        Ok(value) => tool_result(&value.to_string(), false),
        Err(error) => tool_result(&format!("{}: {}", error.code, error.message), true),
    })
}

fn tool_result(text: &str, is_error: bool) -> Value {
    json!({"content": [{"type": "text", "text": text}], "isError": is_error})
}

fn error(id: Value, code: i32, message: &str) -> Value {
    json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}})
}

fn read_line(input: &mut impl BufRead, line: &mut Vec<u8>) -> io::Result<bool> {
    line.clear();
    loop {
        let available = input.fill_buf()?;
        if available.is_empty() {
            return if line.is_empty() {
                Ok(false)
            } else {
                Err(io::Error::new(
                    io::ErrorKind::UnexpectedEof,
                    "Incomplete MCP frame",
                ))
            };
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let take = newline.map_or(available.len(), |index| index + 1);
        if line.len() + take > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP frame exceeds limit",
            ));
        }
        line.extend_from_slice(&available[..take]);
        input.consume(take);
        if newline.is_some() {
            return Ok(true);
        }
    }
}

fn write_response(output: &mut impl Write, response: Value) -> io::Result<()> {
    let mut buffer = LimitedResponse(Vec::new());
    if let Err(failure) = serde_json::to_writer(&mut buffer, &response) {
        if failure.io_error_kind() != Some(io::ErrorKind::InvalidData) {
            return Err(io::Error::other(failure));
        }
        buffer.0.clear();
        let error = error(response["id"].clone(), -32603, "MCP response exceeds limit");
        serde_json::to_writer(&mut buffer, &error).map_err(io::Error::other)?;
    }
    output.write_all(&buffer.0)?;
    output.write_all(b"\n")?;
    output.flush()
}

struct LimitedResponse(Vec<u8>);

impl Write for LimitedResponse {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() >= MAX_FRAME_BYTES - self.0.len() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "MCP response exceeds limit",
            ));
        }
        self.0.write(bytes)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}
