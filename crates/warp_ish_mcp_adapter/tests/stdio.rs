use std::io::{self, BufReader, Cursor};

use serde_json::{Map, Value, json};
use warp_ish_mcp_adapter::{Dispatcher, MAX_FRAME_BYTES, Tool, ToolError, serve};

#[derive(Default)]
struct Recorder {
    calls: Vec<(Tool, Map<String, Value>)>,
    payload: Option<String>,
}

impl Dispatcher for Recorder {
    fn call(&mut self, tool: Tool, arguments: Map<String, Value>) -> Result<Value, ToolError> {
        self.calls.push((tool, arguments));
        match tool {
            Tool::ClosePane => Err(ToolError {
                code: "stale_target".into(),
                message: "Pane is closed".into(),
            }),
            _ => Ok(json!({"pane_id": self.payload.as_deref().unwrap_or("new-pane")})),
        }
    }
}

fn initialize(version: &str) -> Value {
    json!({"jsonrpc":"2.0", "id":1, "method":"initialize", "params":{
        "protocolVersion":version, "capabilities":{},
        "clientInfo":{"name":"test","version":"1"}
    }})
}

fn initialized() -> Value {
    json!({"jsonrpc":"2.0", "method":"notifications/initialized"})
}

fn call(id: u32, tool: &str, arguments: Value) -> Value {
    json!({"jsonrpc":"2.0", "id":id, "method":"tools/call", "params":{
        "name":tool, "arguments":arguments
    }})
}

fn run(messages: Vec<Value>, dispatcher: &mut Recorder) -> Vec<Value> {
    let requests: String = messages
        .into_iter()
        .map(|message| format!("{message}\n"))
        .collect();
    let mut output = Vec::new();
    serve(
        BufReader::with_capacity(12, Cursor::new(requests)),
        &mut output,
        "0.1.0-test",
        dispatcher,
    )
    .unwrap();
    String::from_utf8(output)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn negotiates_both_versions_and_lists_the_static_catalog() {
    for version in ["2024-11-05", "2025-06-18"] {
        let mut dispatcher = Recorder::default();
        let responses = run(
            vec![
                initialize(version),
                initialized(),
                json!({"jsonrpc":"2.0","id":"list","method":"tools/list"}),
            ],
            &mut dispatcher,
        );
        assert_eq!(responses.len(), 2);
        assert_eq!(responses[0]["result"]["protocolVersion"], version);
        assert_eq!(
            responses[0]["result"]["serverInfo"]["version"],
            "0.1.0-test"
        );
        let instructions = responses[0]["result"]["instructions"].as_str().unwrap();
        for term in [
            "`where_am_i`",
            "`split_pane`",
            "`pane.id`",
            "`pane_id`",
            "`start_peer`",
            "idle PowerShell prompt",
            "`send_key`",
            "`close_pane`",
        ] {
            assert!(instructions.contains(term), "missing {term} for {version}");
        }
        assert_eq!(responses[1]["id"], "list");
        let tools = responses[1]["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 8);
        assert_eq!(tools[3]["name"], "split_pane");
        assert_eq!(
            tools[3]["inputSchema"]["required"],
            json!(["pane_id", "direction"])
        );
        assert_eq!(tools[3]["inputSchema"]["additionalProperties"], false);
        assert!(dispatcher.calls.is_empty());
    }
}

#[test]
fn dispatches_typed_calls_once_and_reports_a_backend_error() {
    let mut dispatcher = Recorder::default();
    let split = json!({"pane_id":"source-pane", "direction":"right"});
    let close = json!({"pane_id":"closed-pane"});
    let responses = run(
        vec![
            call(2, "split_pane", split.clone()),
            initialize("2025-06-18"),
            call(3, "split_pane", split.clone()),
            initialized(),
            call(4, "split_pane", split.clone()),
            call(5, "close_pane", close.clone()),
        ],
        &mut dispatcher,
    );
    assert_eq!(responses[0]["error"]["code"], -32002);
    assert_eq!(responses[2]["error"]["code"], -32002);
    assert_eq!(responses[3]["result"]["isError"], false);
    assert_eq!(
        responses[3]["result"]["content"][0]["text"],
        "{\"pane_id\":\"new-pane\"}"
    );
    assert_eq!(responses[4]["result"]["isError"], true);
    assert_eq!(
        responses[4]["result"]["content"][0]["text"],
        "stale_target: Pane is closed"
    );
    assert_eq!(
        dispatcher.calls,
        vec![
            (Tool::SplitPane, split.as_object().unwrap().clone()),
            (Tool::ClosePane, close.as_object().unwrap().clone()),
        ]
    );
}

#[test]
fn rejects_invalid_arguments_without_forwarding_to_ipc() {
    let mut dispatcher = Recorder::default();
    let responses = run(
        vec![
            initialize("future-version"),
            initialized(),
            call(
                2,
                "split_pane",
                json!({"pane_id":"x", "direction":"forwards"}),
            ),
            call(
                3,
                "send_key",
                json!({"pane_id":"x", "key":"escape", "modifiers":["win"]}),
            ),
            call(
                4,
                "start_peer",
                json!({"pane_id":"x", "program":"cli", "args":[42]}),
            ),
            call(5, "close_tab", json!({"tab_id":"x", "profile":"other"})),
            call(6, "unknown", json!({})),
            json!({"jsonrpc":"2.0", "method":"tools/call", "params":{
                "name":"close_pane", "arguments":{"pane_id":"no-id"}
            }}),
        ],
        &mut dispatcher,
    );
    assert_eq!(responses[0]["result"]["protocolVersion"], "2025-06-18");
    assert_eq!(responses.len(), 6);
    assert!(
        responses[1..5]
            .iter()
            .all(|response| response["result"]["isError"] == true)
    );
    assert_eq!(responses[5]["error"]["code"], -32602);
    assert!(dispatcher.calls.is_empty());
}

#[test]
fn rejects_oversized_or_incomplete_input_without_a_partial_reply() {
    for (bytes, kind) in [
        (vec![b'x'; MAX_FRAME_BYTES + 1], io::ErrorKind::InvalidData),
        (b"{}".to_vec(), io::ErrorKind::UnexpectedEof),
    ] {
        let mut output = Vec::new();
        let failure = serve(
            BufReader::new(Cursor::new(bytes)),
            &mut output,
            "1",
            &mut Recorder::default(),
        )
        .unwrap_err();
        assert_eq!(failure.kind(), kind);
        assert!(output.is_empty());
    }
}

#[test]
fn serializes_oversized_backend_output_as_one_error_frame() {
    let mut dispatcher = Recorder {
        payload: Some("x".repeat(MAX_FRAME_BYTES)),
        ..Recorder::default()
    };
    let responses = run(
        vec![
            initialize("2025-06-18"),
            initialized(),
            call(2, "where_am_i", json!({})),
        ],
        &mut dispatcher,
    );
    assert_eq!(responses.len(), 2);
    assert_eq!(responses[1]["id"], 2);
    assert_eq!(responses[1]["error"]["code"], -32603);
    assert_eq!(dispatcher.calls.len(), 1);
}
