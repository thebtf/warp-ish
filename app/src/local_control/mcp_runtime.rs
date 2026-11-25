use ::local_control::{ControlError, ErrorCode, InstanceId};
use instant::Instant;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::runtime::{Builder, Runtime};
use tokio::sync::Semaphore;
use warp_core::channel::ChannelState;
use warp_ish_mcp_transport::{DISPATCH_TIMEOUT, PipeServer, ToolError};
use warpui::{ModelContext, SingletonEntity};

use super::{LocalControlBridge, LocalControlServer};

pub(super) struct McpServer {
    pub(super) endpoint: String,
    _runtime: Runtime,
}

struct CancelOnDrop(Arc<AtomicBool>);

impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        self.0.store(true, Ordering::Release);
    }
}

impl McpServer {
    pub(super) fn start(ctx: &mut ModelContext<LocalControlServer>) -> Result<Self, ControlError> {
        let runtime = Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .thread_name("warp-ish MCP")
            .build()
            .map_err(unavailable)?;
        let profile =
            ChannelState::windows_warp_ish_data_profile().unwrap_or_else(|| "default".into());
        let pipe = {
            let _enter = runtime.enter();
            PipeServer::bind(&profile).map_err(unavailable)?
        };
        let endpoint = pipe.endpoint().to_owned();
        let identity = InstanceId(uuid::Uuid::new_v4().simple().to_string());
        let spawner = LocalControlBridge::handle(ctx).update(ctx, |bridge, ctx| {
            bridge.set_instance_id(identity.clone());
            ctx.spawner()
        });
        runtime.spawn(async move {
            let pending = Arc::new(Semaphore::new(8));
            let result = pipe
                .run(move |request| {
                    let spawner = spawner.clone();
                    let identity = identity.clone();
                    let pending = pending.clone();
                    async move {
                        let permit = pending.try_acquire_owned().map_err(|_| {
                            ToolError::new("busy", "The local MCP UI queue is full")
                        })?;
                        let cancelled = Arc::new(AtomicBool::new(false));
                        let _cancel_on_drop = CancelOnDrop(cancelled.clone());
                        let deadline = Instant::now() + DISPATCH_TIMEOUT;
                        spawner
                            .spawn(move |bridge, ctx| {
                                let _permit = permit;
                                if cancelled.load(Ordering::Acquire) || Instant::now() >= deadline {
                                    return Err(ControlError::new(
                                        ErrorCode::TargetStateConflict,
                                        "MCP request expired before reaching the UI",
                                    ));
                                }
                                bridge.handle_mcp(request, &identity, ctx)
                            })
                            .await
                            .map_err(|_| {
                                ToolError::new(
                                    "bridge_unavailable",
                                    "The warp-ish UI is unavailable",
                                )
                            })?
                            .map_err(|error| {
                                ToolError::new(&error.code.to_string(), &error.message)
                            })
                    }
                })
                .await;
            if let Err(error) = result {
                log::warn!("Local MCP listener stopped: {error}");
            }
        });
        Ok(Self {
            endpoint,
            _runtime: runtime,
        })
    }
}

fn unavailable(error: std::io::Error) -> ControlError {
    ControlError::with_details(
        ErrorCode::TransportUnavailable,
        "Could not start local MCP",
        error.to_string(),
    )
}
