use std::process::ExitCode;

const VERSION: &str = match option_env!("WARP_ISH_RELEASE_VERSION") {
    Some(version) => version,
    None => env!("CARGO_PKG_VERSION"),
};

fn main() -> ExitCode {
    let mut args = std::env::args_os().skip(1);
    match (args.next(), args.next()) {
        (None, None) => run_stdio(),
        (Some(arg), None) if arg == "--version" => {
            println!("warp-ish-mcp {VERSION}");
            ExitCode::SUCCESS
        }
        _ => {
            eprintln!("Usage: warp-ish-mcp [--version]");
            ExitCode::FAILURE
        }
    }
}

#[cfg(windows)]
fn run_stdio() -> ExitCode {
    let mut dispatcher = windows::PipeDispatcher::from_environment();
    match warp_ish_mcp_adapter::serve(
        std::io::stdin().lock(),
        std::io::stdout().lock(),
        VERSION,
        &mut dispatcher,
    ) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("warp-ish-mcp: {error}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(not(windows))]
fn run_stdio() -> ExitCode {
    eprintln!("warp-ish-mcp stdio is available on Windows");
    ExitCode::FAILURE
}

#[cfg(windows)]
mod windows {
    use std::env;

    use serde_json::{Map, Value};
    use tokio::runtime::{Builder, Runtime};
    use warp_ish_mcp_adapter::{Dispatcher, Tool, ToolError};
    use warp_ish_mcp_transport::{
        BLOCK_ID_ENV, PIPE_ENV, PipeClient, Request, TERMINAL_UUID_ENV, VERSION,
    };

    pub(super) struct PipeDispatcher {
        endpoint: Option<String>,
        terminal_uuid: Option<String>,
        block_id: Option<String>,
        runtime: Option<Runtime>,
        pipe: Option<PipeClient>,
    }

    impl PipeDispatcher {
        pub(super) fn from_environment() -> Self {
            Self {
                endpoint: env::var(PIPE_ENV)
                    .ok()
                    .filter(|endpoint| !endpoint.is_empty()),
                terminal_uuid: env::var(TERMINAL_UUID_ENV).ok(),
                block_id: env::var(BLOCK_ID_ENV).ok(),
                runtime: None,
                pipe: None,
            }
        }
    }

    impl Dispatcher for PipeDispatcher {
        fn call(&mut self, tool: Tool, arguments: Map<String, Value>) -> Result<Value, ToolError> {
            let endpoint = self.endpoint.as_deref().ok_or_else(|| ToolError {
                code: "not_attached".into(),
                message: "Launch the MCP client from a warp-ish pane with local control enabled."
                    .into(),
            })?;
            let runtime = match self.runtime.as_ref() {
                Some(runtime) => runtime,
                None => self.runtime.insert(
                    Builder::new_current_thread()
                        .enable_io()
                        .enable_time()
                        .build()
                        .map_err(|_| ToolError {
                            code: "runtime_unavailable".into(),
                            message: "Could not initialize local control.".into(),
                        })?,
                ),
            };
            let pipe = match self.pipe.as_mut() {
                Some(pipe) => pipe,
                None => self.pipe.insert(
                    runtime
                        .block_on(PipeClient::connect(endpoint))
                        .map_err(|_| ToolError {
                            code: "instance_unavailable".into(),
                            message: "The selected warp-ish instance is not accepting local control requests.".into(),
                        })?,
                ),
            };
            let request = Request {
                version: VERSION,
                terminal_uuid: self.terminal_uuid.clone(),
                block_id: self
                    .block_id
                    .clone()
                    .filter(|_| tool.name() == "where_am_i"),
                tool: tool.name().into(),
                arguments,
            };
            match runtime.block_on(pipe.request(&request)) {
                Ok(result) => result.map_err(|error| ToolError {
                    code: error.code,
                    message: error.message,
                }),
                Err(_) => {
                    self.pipe = None;
                    Err(ToolError {
                        code: "request_unconfirmed".into(),
                        message: "Inspect warp-ish before retrying a state-changing request."
                            .into(),
                    })
                }
            }
        }
    }
}
