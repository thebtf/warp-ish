#![cfg(windows)]

use std::{
    io::Write,
    os::windows::process::CommandExt,
    process::{Command, Stdio},
};

use serde_json::{Value, json};
use warp_ish_mcp_transport::{PIPE_ENV, PipeServer, TERMINAL_UUID_ENV};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const CALLER: &str = "0123456789abcdef0123456789abcdef";

fn invoke(endpoint: Option<String>) -> Vec<Value> {
    let mut command = Command::new(env!("CARGO_BIN_EXE_warp-ish-mcp"));
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env(TERMINAL_UUID_ENV, CALLER)
        .creation_flags(CREATE_NO_WINDOW);
    if let Some(endpoint) = endpoint {
        command.env(PIPE_ENV, endpoint);
    } else {
        command.env_remove(PIPE_ENV);
    }
    let mut child = command.spawn().unwrap();
    let mut input = child.stdin.take().unwrap();
    for message in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2024-11-05", "capabilities":{},
            "clientInfo":{"name":"fixture","version":"1"}
        }}),
        json!({"jsonrpc":"2.0","method":"notifications/initialized"}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"where_am_i"}}),
    ] {
        writeln!(input, "{message}").unwrap();
    }
    drop(input);

    let output = child.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[tokio::test]
async fn binary_forwards_inherited_caller_identity_over_windows_pipe() {
    let server = PipeServer::bind("adapter-test").unwrap();
    let endpoint = server.endpoint().to_owned();
    let serving = tokio::spawn(server.run(|request| async move {
        assert_eq!(request.tool, "where_am_i");
        assert!(request.arguments.is_empty());
        Ok(json!({"terminal_uuid": request.terminal_uuid}))
    }));

    let responses = tokio::task::spawn_blocking(move || invoke(Some(endpoint)))
        .await
        .unwrap();
    serving.abort();
    assert_eq!(responses.len(), 2);
    let result = &responses[1]["result"];
    assert_eq!(result["isError"], false);
    let caller: Value =
        serde_json::from_str(result["content"][0]["text"].as_str().unwrap()).unwrap();
    assert_eq!(caller["terminal_uuid"], CALLER);
}

#[test]
fn an_unattached_binary_reports_an_error_at_call_time() {
    let responses = invoke(None);
    assert_eq!(responses[0]["result"]["protocolVersion"], "2024-11-05");
    assert_eq!(responses[1]["result"]["isError"], true);
    assert!(
        responses[1]["result"]["content"][0]["text"]
            .as_str()
            .unwrap()
            .starts_with("not_attached:")
    );
}
