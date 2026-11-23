#![allow(deprecated)]

use std::ffi::{CString, OsString};
use std::os::unix::ffi::OsStrExt as _;
use std::os::unix::fs::MetadataExt;
use std::os::unix::io::AsRawFd as _;
use std::path::{Path, PathBuf};
use std::fs;

use anyhow::{Context, Result, anyhow, bail, ensure};
use channel_versions::VersionInfo;
use command::r#async::Command;
use command::blocking;
use futures::StreamExt;
use instant::Instant;
use nix::errno::Errno;
use nix::unistd::{fchown, getgid, getuid};
use warp_core::macos::get_bundle_path;
use warp_core::safe_error;
use warp_errors::report_error;
use warpui::{AppContext, ModelContext, SingletonEntity};

use crate::appearance::AppearanceManager;
use crate::autoupdate::{AutoupdateStage, AutoupdateState};
use crate::channel::{Channel, ChannelState};
use crate::safe_info;

// Relative path to the directory containing old executables from before an autoupdate.
//
// TODO(vorporeal): This and relevant code should be deleted after auto-updates have been
//      storing the old executable in the user application data directory for a couple
//      releases.
const OLD_EXECUTABLE_PATH: &str = "Contents/MacOS/old";

// Name of the old executable file that was kept around during an autoupdate.
//
// TODO: This and relevant code should be deleted after auto-updates have been keeping the whole
//      old app bundle (see `OLD_BUNDLE_NAME`) for a couple releases.
const OLD_EXECUTABLE_FILE_NAME: &str = "old";

// Name of the old app bundle that is kept around after an autoupdate until Warp relaunches.
const OLD_BUNDLE_NAME: &str = "old.app";

// Tmp file name used to check if the user has the correct permissions for autoupdate.
const PERMISSIONS_TMP_FILE_NAME: &str = "permission_test";

fn old_executable_file_path() -> PathBuf {
    warp_core::paths::state_dir().join(OLD_EXECUTABLE_FILE_NAME)
}

fn old_bundle_path() -> PathBuf {
    warp_core::paths::state_dir().join(OLD_BUNDLE_NAME)
}

/// Removes the previous version of the app that was kept around by an autoupdate so that it could
/// keep running until relaunch. This is called when the app is first launched.
pub(super) fn remove_old_executable() -> Result<()> {
    // TODO(vorporeal): This code should be deleted after auto-updates have been
    //      storing the old executable in the user application data directory for
    //      a couple releases.
    log::info!("Removing old executable dir...");
    let old_executable_path = PathBuf::from(get_bundle_path()?).join(OLD_EXECUTABLE_PATH);
    if let Ok(metadata) = fs::metadata(&old_executable_path)
        && metadata.is_dir()
    {
        fs::remove_dir_all(old_executable_path)?;
    }

    log::info!("Removing old executable file...");
    let old_executable_file_path = old_executable_file_path();
    if let Ok(metadata) = fs::metadata(&old_executable_file_path)
        && metadata.is_file()
    {
        fs::remove_file(old_executable_file_path)?;
    }

    log::info!("Removing old app bundle...");
    let old_bundle_path = old_bundle_path();
    if let Ok(metadata) = fs::metadata(&old_bundle_path)
        && metadata.is_dir()
    {
        fs::remove_dir_all(old_bundle_path)?;
    }

    Ok(())
}

/// If the autoupdate state is ready, asynchronously apply the update and cleanup the autoupdate artifacts.
///
/// The completion callback is invoked with `Ok(Some(version))` if an update was applied, and `Ok(None)` if there was no update.
/// If there was an update, but applying it failed, it's invoked with `Err(err)`.
pub(super) fn apply_update_async<F>(app: &mut AppContext, callback: F)
where
    F: FnOnce(
            &mut AutoupdateState,
            Result<Option<VersionInfo>>,
            &mut ModelContext<AutoupdateState>,
        ) + Send
        + 'static,
{
    AutoupdateState::handle(app).update(app, |autoupdate_state, ctx| {
        match autoupdate_state.stage.clone() {
            AutoupdateStage::UpdateReady {
                new_version,
                update_id,
            }
            | AutoupdateStage::Updating {
                new_version,
                update_id,
            } => {
                let update_id_clone = update_id.clone();
                // Apply the update in a background thread.
                ctx.spawn(
                    async move {
                        let result =
                            apply_update(ChannelState::channel(), &new_version, &update_id)
                                .await
                                .map(|_| Some(new_version));
                        cleanup(&update_id).await;
                        result
                    },
                    move |autoupdate_state, result, ctx| {
                        if result.is_ok() {
                            // Reset app icon to previously selected app icon
                            AppearanceManager::as_ref(ctx).set_app_icon(ctx);
                        }
                        autoupdate_state.clear_downloaded_update(&update_id_clone, ctx);
                        callback(autoupdate_state, result, ctx);
                    },
                );
            }
            _ => {
                callback(autoupdate_state, Ok(None), ctx);
            }
        }
    })
}

pub(super) fn relaunch() -> Result<()> {
    let bundle_path = PathBuf::from(get_bundle_path()?);
    // Set the -n option to open a new instance of the app even if one is
    // running so we still launch the new version even if the user was running
    // multiple instances of Warp.
    let mut launch_command = OsString::from("/usr/bin/open -n ");
    launch_command.push(bundle_path.as_os_str());
    // Pass a flag to the app to let it know it was restarted as part of the
    // autoupdate process.
    launch_command.push(format!(" --args {}", warp_cli::finish_update_flag()));
    // We need to make sure that the current Warp process is no longer running
    // before we spawn the new one, otherwise we can end up showing multiple
    // icons in the macOS dock.  To do this, we use an intermediary /bin/sh
    // process that watches for this process to terminate, and then spawns a
    // new Warp process.
    //
    // Wait until the current process is no longer running, checking every
    // 200ms.  Once the current process has terminated, launch the new one.
    let pid = std::process::id();
    let mut relaunch_command = OsString::from(format!(
        "while ps -p {pid} >/dev/null 2>&1; do sleep 0.2; done; "
    ));
    relaunch_command.push(launch_command);

    log::info!("Executing relaunch command {relaunch_command:?}");
    blocking::Command::new("sh")
        .arg("-c")
        .arg(relaunch_command)
        .spawn()?;
    Ok(())
}

pub async fn cleanup(update_id: &str) {
    let download_dir = get_download_dir(update_id);
    if download_dir.exists() {
        log::info!("Cleaning up download dir {:?}", &download_dir);
        if let Err(e) = async_fs::remove_dir_all(&download_dir).await {
            safe_error!(
                safe: ("Error cleaning up download dir: {e:?}"),
                full: ("Error cleaning up download dir {:?}: {:?}", &download_dir, e)
            );
        }
    }
}

/// Clean up all autoupdate directories except the specified one.
/// This helps prevent accumulation of old update directories from failed downloads,
/// race conditions, or incomplete cleanups.
pub async fn cleanup_all_except(preserve_update_id: Option<&str>) {
    let mut autoupdate_dir = warp_core::paths::cache_dir();
    autoupdate_dir.push("autoupdate");

    if !autoupdate_dir.exists() {
        return;
    }

    log::debug!("Cleaning up all autoupdate directories except {preserve_update_id:?}");

    let mut entries = match async_fs::read_dir(&autoupdate_dir).await {
        Ok(entries) => entries,
        Err(e) => {
            log::warn!("Could not read autoupdate directory {autoupdate_dir:?}: {e:?}");
            return;
        }
    };

    while let Some(entry) = entries.next().await {
        let entry = match entry {
            Ok(entry) => entry,
            Err(e) => {
                log::warn!("Error reading autoupdate directory entry: {e:?}");
                continue;
            }
        };

        let path = entry.path();
        let file_name = match path.file_name().and_then(|n| n.to_str()) {
            Some(name) => name,
            None => continue,
        };

        // Skip the directory we want to preserve
        if let Some(preserve_id) = preserve_update_id
            && file_name == preserve_id
        {
            log::debug!("Preserving autoupdate directory: {path:?}");
            continue;
        }

        let metadata = match async_fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(e) => {
                log::warn!("Could not get metadata for {path:?}: {e:?}");
                continue;
            }
        };

        if metadata.is_dir() {
            log::debug!("Removing old autoupdate directory: {path:?}");
            if let Err(e) = async_fs::remove_dir_all(&path).await {
                log::warn!("Failed to remove autoupdate directory {path:?}: {e:?}");
            }
        }
    }
}

/// Determines if the user needs authorization in order to update Warp.
async fn is_directory_writable(directory: &Path) -> Result<bool> {
    // Just because we have writability access does not mean we can set the correct owner/group.
    // Test if we can set the owner/group on a temporarily created file. If we can, then we can
    // probably perform an update without authorization.
    let tmp_file_name = directory.join(PERMISSIONS_TMP_FILE_NAME);

    safe_info!(
        safe: ("Writing to a tmp file to determine if permissions are correct"),
        full: ("Writing to a tmp file to determine if permissions are correct in {}", directory.display())
    );

    let needs_authorization = match async_fs::File::create(&tmp_file_name).await {
        Ok(file) => {
            let fchown_result = fchown(file.as_raw_fd(), Some(getuid()), Some(getgid()));
            if let Err(err) = &fchown_result {
                log::warn!("Could not set permissions on tmp file: {err:#}");
            }

            // Only remove the tmp file if it was created - otherwise, we'll mask permission
            // errors.
            async_fs::remove_file(&tmp_file_name).await?;
            fchown_result.is_ok()
        }
        Err(e) => {
            // Obvious indicator we may need authorization.
            log::warn!("Could not create tmp file: {e:#}");
            false
        }
    };

    Ok(needs_authorization)
}

/// Verifies that the staged bundle path has a valid macOS code signature, and that its
/// team identifier matches Warp's team identifier.
async fn apply_update(channel: Channel, version_info: &VersionInfo, update_id: &str) -> Result<()> {
    let update_start = Instant::now();

    let bundle_path = PathBuf::from(get_bundle_path()?);
    let bundle_parent_path = bundle_path
        .parent()
        .ok_or_else(|| anyhow!("Could not get parent directory of application bundle"))?;

    // Double-check that we have permissions to apply the update.
    if !permissions::is_writable(&bundle_path)? {
        bail!("App location is not writable, cannot apply update");
    }
    if !is_directory_writable(bundle_parent_path).await? {
        bail!("App parent location is not writable, cannot apply update");
    }

    // Read a file out of the old bundle to ensure that we've triggered macOS' directory
    // permissions checks.
    let old_info_plist = bundle_path.join("Contents/Info.plist");
    if async_fs::File::open(&old_info_plist).await.is_err() {
        bail!("App location is not readable, cannot apply update");
    }

    let dmg_path = dmg_path(&channel, version_info, update_id);
    let temp_app_path = temporary_target_path(channel, version_info, &dmg_path)?;

    let staged_bundle =
        StagedBundle::for_bundle_path(channel, version_info, temp_app_path, &bundle_path).await?;

    // Copy permissions to new app
    let bundle_metadata = async_fs::metadata(&bundle_path).await?;
    async_fs::set_permissions(&staged_bundle.path, bundle_metadata.permissions()).await?;

    // Verify that the new version actually exists before proceeding
    let executable_path_buf = staged_bundle.path.join(executable_path(channel));
    if !executable_path_buf.exists() {
        bail!(
            "New executable does not exist at path: {:?}",
            executable_path_buf
        );
    }

    // Atomically rename the new app to have the same name as the old one.
    log::info!("Renaming new app to original app name");
    let from = CString::new(staged_bundle.path.as_os_str().as_bytes())?;
    let to = CString::new(bundle_path.as_os_str().as_bytes())?;

    Errno::result(unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) })
        .context("Error swapping old and new app bundles")?;

    // After the swap, the old bundle (containing the running executable) is at the staged path.
    preserve_running_bundle(&staged_bundle.path, &old_bundle_path()).await?;

    log::info!("Setting installed version to {:?}", &version_info);
    log::info!("Applied update in {:?}", update_start.elapsed());

    Ok(())
}

/// Moves the app bundle that an update swapped out, which contains the running executable, to
/// `destination` so that it survives cleanup of the update's download directory.
///
/// Removing the running executable causes the `fork` syscall to fail, and Warp keeps running
/// from it if the user cancels quitting after the update is applied. The whole bundle is kept
/// rather than just the executable because macOS attributes privacy permissions (e.g. Full Disk
/// Access) through the executable's signed app bundle; an executable moved out of its bundle
/// loses access to protected locations.
async fn preserve_running_bundle(swapped_out_bundle: &Path, destination: &Path) -> Result<()> {
    if async_fs::metadata(destination).await.is_ok() {
        // If we already applied an update but didn't relaunch Warp, the running executable is
        // still in the bundle at `destination`, so leave it there.
        log::info!("Already autoupdated without relaunching; ignoring old bundle");
        return Ok(());
    }

    log::info!(
        "Moving old bundle at path {swapped_out_bundle:?} into user application data dir at path {destination:?}"
    );
    let mv_output = Command::new("mv")
        .arg(swapped_out_bundle)
        .arg(destination)
        .output()
        .await?;

    ensure!(
        mv_output.status.success(),
        "Failed to move old bundle: {mv_output:?}"
    );

    Ok(())
}

/// The staged app bundle that we're about to install. It's copied out of the `.dmg` file into a
/// temporary location.
struct StagedBundle {
    /// Path to the on-disk temporary bundle.
    path: PathBuf,
    /// Whether or not the temporary bundle was copied into the same directory as the existing app.
    /// This is only necessary if `$TMPDIR` and the app are on different filesystems.
    in_app_directory: bool,
}

impl StagedBundle {
    async fn for_bundle_path(
        channel: Channel,
        version_info: &VersionInfo,
        temp_app_path: PathBuf,
        bundle_path: &Path,
    ) -> Result<Self> {
        let temp_device_id = async_fs::metadata(&temp_app_path)
            .await
            .context("Could not get metadata for temporary app bundle")?
            .dev();
        let bundle_device_id = async_fs::metadata(bundle_path)
            .await
            .context("Could not get metadata for app bundle")?
            .dev();

        if temp_device_id == bundle_device_id {
            // The old and new app bundles are on the same filesystem (this is the expected case).
            Ok(Self {
                path: temp_app_path,
                in_app_directory: false,
            })
        } else {
            let bundle_parent_path = bundle_path
                .parent()
                .ok_or_else(|| anyhow!("Could not get parent directory of application bundle"))?;
            log::info!("Copying app contents from {temp_app_path:?} to {bundle_parent_path:?}");

            let cp_output = Command::new("cp")
                // Recursively copy the directory, preserving symlinks.
                .arg("-R")
                // Overwrite files at the destination.
                .arg("-f")
                .arg(&temp_app_path)
                .arg(bundle_parent_path)
                .output()
                .await?;

            ensure!(
                cp_output.status.success(),
                "Failed to copy app contents from temporary directory into bundle directory: {cp_output:?}"
            );

            Ok(Self {
                path: bundle_parent_path.join(versioned_app_name(channel, &version_info.version)),
                in_app_directory: true,
            })
        }
    }
}

impl Drop for StagedBundle {
    fn drop(&mut self) {
        // Clean up in the destructor so that it happens even if the installation errors.
        // If we used the original temporary app bundle, it'll get removed by the final cleanup
        // step, along with the dmg. Once the update is applied, this path holds the old bundle,
        // which may have already been moved away by `preserve_running_bundle`.
        if self.in_app_directory && self.path.exists() {
            log::info!("Removing temporary app bundle");
            if let Err(err) = fs::remove_dir_all(&self.path) {
                report_error!(anyhow::Error::new(err).context("Failed to remove temporary bundle"));
            }
        }
    }
}

/// The temporary path for downloading the new dmg into.
fn dmg_path(channel: &Channel, version_info: &VersionInfo, update_id: &str) -> PathBuf {
    let mut dir = get_download_dir(update_id);
    let file_name = format!(
        "{}.{}.dmg",
        &version_info.version,
        app_name_prefix(*channel)
    );
    dir.push(file_name);
    dir
}

/// The temporary path for placing our downloaded app binary.
fn temporary_target_path(
    channel: Channel,
    version_info: &VersionInfo,
    dmg_path: &Path,
) -> Result<PathBuf> {
    Ok(dmg_path
        .parent()
        .ok_or_else(|| anyhow!("Could not get parent directory of downloaded DMG"))?
        .join(versioned_app_name(channel, &version_info.version)))
}

fn get_download_dir(update_id: &str) -> PathBuf {
    let mut dir = warp_core::paths::cache_dir();
    dir.push("autoupdate");
    dir.push(update_id);
    dir
}

fn app_name(channel: Channel) -> String {
    format!("{}.app", app_name_prefix(channel))
}

fn versioned_app_name(channel: Channel, version: &str) -> String {
    format!("{}({}).app", app_name_prefix(channel), version)
}

fn app_name_prefix(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "Warp",
        Channel::Preview => "WarpPreview",
        Channel::Local => "warp",
        Channel::Integration => "integration",
        Channel::Dev => "WarpDev",
        Channel::Oss => "warp-oss",
    }
}

fn executable_name(channel: Channel) -> &'static str {
    match channel {
        Channel::Stable => "stable",
        Channel::Preview => "preview",
        Channel::Local => "warp",
        Channel::Integration => "integration",
        Channel::Dev => "dev",
        Channel::Oss => "warp-oss",
    }
}

fn executable_path(channel: Channel) -> String {
    if ChannelState::is_release_bundle() {
        format!("Contents/MacOS/{}", executable_name(channel))
    } else {
        executable_name(channel).to_owned()
    }
}
