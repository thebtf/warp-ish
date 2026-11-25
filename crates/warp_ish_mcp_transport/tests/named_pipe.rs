#![cfg(windows)]

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

use serde_json::{Map, Value};
use tokio::sync::Barrier;
use tokio::task::JoinSet;
use tokio::time::Instant;
use warp_ish_mcp_transport::{
    MAX_FRAME_BYTES, PipeClient, PipeServer, Request, ToolError, VERSION,
};

fn request(tool: &str) -> Request {
    Request {
        block_id: None,
        version: VERSION,
        terminal_uuid: Some("fixture-pane".into()),
        tool: tool.into(),
        arguments: Map::new(),
    }
}

#[tokio::test]
async fn forty_clients_open_concurrent_requests_on_one_pipe() {
    let server = PipeServer::bind("test-profile-40").unwrap();
    let endpoint = server.endpoint().to_owned();
    let running = Arc::new(AtomicUsize::new(0));
    let peak = Arc::new(AtomicUsize::new(0));
    let observe_peak = peak.clone();
    let task = tokio::spawn(server.run(move |request| {
        let running = running.clone();
        let peak = peak.clone();
        async move {
            peak.fetch_max(running.fetch_add(1, Ordering::SeqCst) + 1, Ordering::SeqCst);
            tokio::time::sleep(Duration::from_millis(5)).await;
            running.fetch_sub(1, Ordering::SeqCst);
            assert_eq!(request.terminal_uuid.as_deref(), Some("fixture-pane"));
            Ok(Value::String(request.tool))
        }
    }));
    let mut clients = JoinSet::new();
    let start = Instant::now();
    let barrier = Arc::new(Barrier::new(40));
    for i in 0..40 {
        let endpoint = endpoint.clone();
        let barrier = barrier.clone();
        clients.spawn(async move {
            let mut client = PipeClient::connect(&endpoint).await.unwrap();
            barrier.wait().await;
            for sample in 0..2 {
                let tool = format!("test-{i}-{sample}");
                let response = client.request(&request(&tool)).await.unwrap().unwrap();
                assert_eq!(response, Value::String(tool));
            }
        });
    }
    tokio::time::timeout(Duration::from_secs(12), async {
        while let Some(result) = clients.join_next().await {
            result.unwrap();
        }
    })
    .await
    .unwrap();
    let peak = observe_peak.load(Ordering::SeqCst);
    assert!((2..=8).contains(&peak));
    eprintln!(
        "40 connected, 80 calls, peak {peak}, elapsed {:?}",
        start.elapsed()
    );
    task.abort();
}

#[tokio::test]
async fn idle_client_can_resume_after_frame_timeout() {
    let server = PipeServer::bind("test-profile-idle").unwrap();
    let endpoint = server.endpoint().to_owned();
    let task = tokio::spawn(server.run(|request| async move { Ok(Value::String(request.tool)) }));
    let mut client = PipeClient::connect(&endpoint).await.unwrap();
    assert_eq!(
        client.request(&request("before")).await.unwrap().unwrap(),
        "before"
    );
    // Longer than the server's three-second partial-frame timeout.
    tokio::time::sleep(Duration::from_secs(4)).await;
    assert_eq!(
        client.request(&request("after")).await.unwrap().unwrap(),
        "after"
    );
    task.abort();
}

#[tokio::test]
async fn reports_unsupported_version_and_oversize_reply_without_poisoning_connection() {
    let server = PipeServer::bind("test-profile-errors").unwrap();
    let mut client = PipeClient::connect(server.endpoint()).await.unwrap();
    let task = tokio::spawn(server.run(|request| async move {
        match request.tool.as_str() {
            "large" => Ok(Value::String("a".repeat(MAX_FRAME_BYTES))),
            "none" => Ok(Value::Null),
            _ => Err(ToolError::new("unknown_tool", "Unknown tool")),
        }
    }));
    let mut stale = request("none");
    stale.version += 1;
    let error = client.request(&stale).await.unwrap().unwrap_err();
    assert_eq!(error.code, "unsupported_version");
    let error = client
        .request(&request("large"))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(error.code, "response_too_large");
    assert_eq!(
        client.request(&request("none")).await.unwrap().unwrap(),
        Value::Null
    );
    task.abort();
}

#[tokio::test]
async fn rejects_remote_endpoints_and_invalid_profile_names() {
    assert!(PipeServer::bind("../invalid").is_err());
    assert!(
        PipeClient::connect(r"\\otherPC\pipe\warp-ish-mcp-v1-fake")
            .await
            .is_err()
    );
}
