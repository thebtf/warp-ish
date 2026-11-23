//! Local Git root discovery for terminal sessions.

use std::future::{Future, ready};
use std::path::PathBuf;

#[cfg(not(target_family = "wasm"))]
use futures::future::Either;
#[cfg(not(target_family = "wasm"))]
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::repositories::RepoDetectionSource;
use warpui::AppContext;
#[cfg(not(target_family = "wasm"))]
use warpui::SingletonEntity;

/// Detects the Git root from a local terminal session's working directory.
/// An SSH directory may share a name with a local directory, so it is never
/// passed to the local filesystem lookup.
#[cfg(not(target_family = "wasm"))]
pub fn detect_possible_git_repo(
    is_local_session: bool,
    active_directory: &str,
    source: RepoDetectionSource,
    ctx: &mut AppContext,
) -> impl Future<Output = Option<PathBuf>> + use<> {
    if !is_local_session {
        return Either::Right(ready(None));
    }

    Either::Left(DetectedRepositories::handle(ctx).update(ctx, |repos, ctx| {
        repos.detect_possible_local_git_repo(active_directory, source, ctx)
    }))
}

/// The native repository watcher is unavailable in WASM builds.
#[cfg(target_family = "wasm")]
pub fn detect_possible_git_repo(
    _is_local_session: bool,
    _active_directory: &str,
    _source: RepoDetectionSource,
    _ctx: &mut AppContext,
) -> impl Future<Output = Option<PathBuf>> + use<> {
    ready(None)
}
