use std::io;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

#[cfg(windows)]
mod pipe;
#[cfg(windows)]
pub use pipe::{DISPATCH_TIMEOUT, PipeClient, PipeServer};

pub const PIPE_ENV: &str = "WARP_ISH_MCP_PIPE";
pub const TERMINAL_UUID_ENV: &str = "WARP_TERMINAL_SESSION_UUID";
pub const BLOCK_ID_ENV: &str = "WARP_ISH_MCP_BLOCK_ID";
pub const VERSION: u32 = 1;
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub version: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_uuid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub block_id: Option<String>,
    pub tool: String,
    pub arguments: Map<String, Value>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ToolError {
    pub code: String,
    pub message: String,
}

impl ToolError {
    pub fn new(code: &str, message: &str) -> Self {
        Self {
            code: code.into(),
            message: message.into(),
        }
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Reply {
    ok: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    result: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    error: Option<ToolError>,
}

impl Reply {
    fn new(result: Result<Value, ToolError>) -> Self {
        match result {
            Ok(result) => Self {
                ok: true,
                result: Some(result),
                error: None,
            },
            Err(error) => Self {
                ok: false,
                result: None,
                error: Some(error),
            },
        }
    }

    fn into_result(self) -> io::Result<Result<Value, ToolError>> {
        match (self.ok, self.result, self.error) {
            // serde treats an explicit JSON null as None for Option<Value>.
            (true, result, None) => Ok(Ok(result.unwrap_or(Value::Null))),
            (false, None, Some(error)) => Ok(Err(error)),
            _ => Err(invalid_data("Invalid response envelope")),
        }
    }
}

fn invalid_data(message: &str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

async fn read_frame<R: AsyncBufRead + Unpin>(reader: &mut R) -> io::Result<Option<Vec<u8>>> {
    let mut frame = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return if frame.is_empty() {
                Ok(None)
            } else {
                Err(invalid_data("Incomplete frame"))
            };
        }
        let newline = available.iter().position(|&byte| byte == b'\n');
        let length = newline.map_or(available.len(), |offset| offset + 1);
        if frame.len() + length > MAX_FRAME_BYTES {
            return Err(invalid_data("Frame exceeds the maximum size"));
        }
        frame.extend_from_slice(&available[..length]);
        reader.consume(length);
        if newline.is_some() {
            frame.pop();
            return Ok(Some(frame));
        }
    }
}

fn encode_frame<T: Serialize>(value: &T) -> io::Result<Vec<u8>> {
    struct LimitedFrame(Vec<u8>);

    impl io::Write for LimitedFrame {
        fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
            if bytes.len() >= MAX_FRAME_BYTES - self.0.len() {
                return Err(invalid_data("Frame exceeds the maximum size"));
            }
            self.0.extend_from_slice(bytes);
            Ok(bytes.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    let mut frame = LimitedFrame(Vec::new());
    serde_json::to_writer(&mut frame, value).map_err(io::Error::from)?;
    frame.0.push(b'\n');
    Ok(frame.0)
}

async fn write_frame<W: AsyncWrite + Unpin, T: Serialize>(
    writer: &mut W,
    value: &T,
) -> io::Result<()> {
    writer.write_all(&encode_frame(value)?).await?;
    writer.flush().await
}
