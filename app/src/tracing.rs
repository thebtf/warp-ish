use tracing::subscriber;

pub struct Initialization;

pub fn init() -> anyhow::Result<Initialization> {
    // Local logging is handled by warp_logging.
    subscriber::set_global_default(subscriber::NoSubscriber::new())?;
    Ok(Initialization)
}

impl Initialization {
    pub(crate) fn shutdown(&mut self) {}
}
