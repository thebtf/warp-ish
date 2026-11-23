use std::ffi::OsStr;
use std::sync::OnceLock;
use std::{env, path};

use anyhow::{Result, anyhow};
use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System};

const KASPERSKY_PROCESS_NAME: &str = "avp";

#[cfg(feature = "local_fs")]
pub fn install_dir() -> Result<path::PathBuf> {
    let current_exe = env::current_exe()?;
    current_exe
        .parent()
        .map(ToOwned::to_owned)
        .ok_or(anyhow!("Unable to get install dir"))
}

static KASPERSKY_RUNNING: OnceLock<bool> = OnceLock::new();

/// Determines if Kaspersky is currently running by checking if there is a
/// process with the name "avp" running.
///
/// The result is cached for the lifetime of the process: antivirus presence does not meaningfully
/// change mid-session, and the full process-table enumeration this requires is expensive enough on
/// Windows that repeating it per session bootstrap can trip the DPC watchdog.
pub fn is_kaspersky_running() -> bool {
    *KASPERSKY_RUNNING.get_or_init(|| {
        let mut system = System::new();
        // Per-process CPU sampling can trip the Windows DPC watchdog.
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing(),
        );
        system
            .processes_by_name(OsStr::new(KASPERSKY_PROCESS_NAME))
            .next()
            .is_some()
    })
}
