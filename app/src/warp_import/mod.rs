use std::fs;
use std::io::Write as _;
use std::os::windows::fs::MetadataExt as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, ensure};
use directories::ProjectDirs;
use tempfile::NamedTempFile;
use warp_core::AppId;
use warp_core::channel::ChannelState;
use warp_core::paths::{
    config_local_dir, project_dirs_for_app_id, valid_windows_import_profile,
    windows_import_profile_pointer,
};

mod settings;

use self::settings::{SettingsPaths, project_local_files};

const DATABASE_NAME: &str = "warp.sqlite";
const READY_FILE: &str = "warp-import-ready";
const IMPORT_LAUNCH_ENV: &str = "WARP_ISH_IMPORT_LAUNCH";
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

fn settings_paths(dirs: &ProjectDirs) -> SettingsPaths {
    SettingsPaths {
        settings_toml: dirs.config_local_dir().join("settings.toml"),
        keybindings_yaml: dirs.config_local_dir().join("keybindings.yaml"),
        data_dir: dirs.data_dir().to_owned(),
    }
}

fn profile_root(data: &Path) -> Result<&Path> {
    data.parent()
        .context("Could not resolve the workspace profile root")
}

fn validate_directories(path: &Path) -> Result<()> {
    for directory in path.ancestors().filter(|directory| directory.has_root()) {
        match fs::symlink_metadata(directory) {
            Ok(metadata) => ensure!(
                metadata.is_dir() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
                "An import directory is redirected or is not a directory"
            ),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
            Err(error) => return Err(error).context("Checking the import directories"),
        }
    }
    Ok(())
}

struct ReservedProfile {
    roots: Vec<PathBuf>,
    launched: bool,
}

impl Drop for ReservedProfile {
    fn drop(&mut self) {
        if self.launched {
            return;
        }
        for root in &self.roots {
            if validate_directories(root).is_err() {
                continue;
            }
            if let Err(error) = fs::remove_dir_all(root) {
                log::warn!("Could not remove the unused Warp import profile: {error}");
            }
        }
    }
}

fn reserve_profile(local: &Path, roaming: &Path) -> Result<ReservedProfile> {
    let mut reservation = ReservedProfile {
        roots: Vec::new(),
        launched: false,
    };
    let roots = [Some(local), (roaming != local).then_some(roaming)];
    for root in roots.into_iter().flatten() {
        let parent = root
            .parent()
            .context("Could not resolve the profile parent")?;
        validate_directories(parent)?;
        fs::create_dir_all(parent).context("Could not prepare the profile parent")?;
        fs::create_dir(root).context("Could not reserve a fresh import profile")?;
        reservation.roots.push(root.to_owned());
    }
    Ok(reservation)
}

fn select_profile(path: &Path, profile: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("Could not resolve the profile selector")?;
    validate_directories(parent)?;
    match fs::symlink_metadata(path) {
        Ok(metadata) => ensure!(
            metadata.is_file() && metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT == 0,
            "The profile selector is not an ordinary file"
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => (),
        Err(error) => return Err(error).context("Checking the profile selector"),
    }
    fs::create_dir_all(parent).context("Could not prepare the profile selector")?;
    let mut temp = NamedTempFile::new_in(parent)?;
    writeln!(temp, "{profile}")?;
    temp.as_file().sync_all()?;
    temp.persist(path)
        .context("Could not select the imported profile")?;
    Ok(())
}

fn receipt_path(dirs: &ProjectDirs) -> PathBuf {
    dirs.config_local_dir().join("warp-import-previous-profile")
}

fn default_selector(explicit_profile: Option<&str>) -> Result<Option<PathBuf>> {
    if explicit_profile.is_some_and(|profile| !valid_windows_import_profile(profile)) {
        return Ok(None);
    }
    let pointer = windows_import_profile_pointer().context("Could not locate the default profile")?;
    if let Some(profile) = explicit_profile
        && !fs::read_to_string(&pointer).is_ok_and(|value| value.trim() == profile)
    {
        return Ok(None);
    }
    Ok(Some(pointer))
}

/// A child launched for this import confirms only after saved windows opened.
pub(crate) fn confirm_restored_workspace() {
    let Ok(expected) = std::env::var(IMPORT_LAUNCH_ENV) else {
        return;
    };
    if !valid_windows_import_profile(&expected)
        || ChannelState::windows_warp_ish_data_profile().as_deref() != Some(&expected)
    {
        return;
    }
    if let Err(error) = fs::write(config_local_dir().join(READY_FILE), std::process::id().to_string()) {
        log::warn!("Could not confirm the restored Warp workspace: {error}");
    }
}

pub(super) fn run() -> Result<String> {
    let explicit_profile = std::env::var("WARP_DATA_PROFILE").ok();
    let donor_profile = explicit_profile
        .as_deref()
        .filter(|name| !valid_windows_import_profile(name));
    let donor = project_dirs_for_app_id(AppId::new("dev", "warp", "Warp"), donor_profile)
        .context("Could not locate the installed Warp profile")?;
    let donor_db = donor.data_local_dir().join(DATABASE_NAME);
    ensure!(
        donor_db.is_file(),
        "No saved workspace from Warp Stable was found"
    );

    let profile = format!("import-{}", uuid::Uuid::new_v4().simple());
    let staging = project_dirs_for_app_id(AppId::windows_warp_ish(), Some(&profile))
        .context("Could not locate the warp-ish profile")?;
    let local = profile_root(staging.data_local_dir())?;
    let roaming = profile_root(staging.data_dir())?;
    let mut reservation = reserve_profile(local, roaming)?;
    fs::create_dir_all(staging.config_local_dir())?;
    fs::create_dir_all(staging.data_local_dir())?;
    fs::create_dir_all(staging.data_dir())?;

    let source_paths = settings_paths(&donor);
    let destination_paths = settings_paths(&staging);
    let files = project_local_files(&source_paths, &destination_paths, &[local, roaming])?;
    let layout = crate::persistence::import_terminal_layout(
        &donor_db,
        &staging.data_local_dir().join(DATABASE_NAME),
    )?;

    let pointer = default_selector(explicit_profile.as_deref())?;
    if let Some(pointer) = &pointer {
        let previous = match fs::read_to_string(pointer) {
            Ok(value) => value,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => String::new(),
            Err(error) => return Err(error).context("Could not read the previous profile"),
        };
        fs::write(receipt_path(&staging), previous)?;
    }

    let report = serde_json::json!({
        "windows": layout.windows,
        "tab_groups": layout.tab_groups,
        "tabs": layout.tabs,
        "panes": layout.panes,
        "history_entries": layout.history_entries,
        "skipped_windows": layout.skipped_windows,
        "skipped_tabs": layout.skipped_tabs,
        "skipped_panes": layout.skipped_panes,
        "skipped_shells": layout.skipped_shells,
        "skipped_cwds": layout.skipped_cwds,
        "files": files,
    });
    fs::write(
        staging.config_local_dir().join("warp-import-report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    let mut child = Command::new(std::env::current_exe()?)
        .env("WARP_DATA_PROFILE", &profile)
        .env(IMPORT_LAUNCH_ENV, &profile)
        .spawn()
        .context("Could not open the imported workspace")?;
    reservation.launched = true;
    let ready = staging.config_local_dir().join(READY_FILE);
    let child_id = child.id().to_string();
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        ensure!(
            child.try_wait()?.is_none(),
            "The imported warp-ish window exited before restoring the saved workspace"
        );
        if fs::read_to_string(&ready).is_ok_and(|value| value == child_id) {
            break;
        }
        ensure!(Instant::now() < deadline, "The imported warp-ish window did not confirm restoration");
        std::thread::sleep(Duration::from_millis(100));
    }
    if let Some(pointer) = &pointer {
        select_profile(pointer, &profile)?;
    }

    Ok(format!(
        "Imported {} windows, {} tabs, {} terminal panes and {} history entries. Skipped {} panes.",
        layout.windows,
        layout.tabs,
        layout.panes,
        layout.history_entries,
        layout.skipped_panes
    ))
}
