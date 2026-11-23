use std::sync::Arc;

use anyhow::Result;
use channel_versions::{Changelog, VersionInfo};

use super::DownloadReady;

#[derive(Clone)]
pub(crate) enum ReleaseSource {
    Unavailable,
    #[allow(dead_code)]
    Local(Arc<dyn LocalReleaseSource>),
}

/// Supplies metadata and prepares artifacts from the locally available release.
#[async_trait::async_trait]
pub(crate) trait LocalReleaseSource: Send + Sync {
    async fn latest_version(&self) -> Result<Option<VersionInfo>>;

    /// Prepare a verified package before reporting readiness. A failed preparation must
    /// leave any previously ready package in place.
    async fn prepare_update(
        &self,
        version: &VersionInfo,
        update_id: &str,
        previous_update_id: Option<&str>,
    ) -> Result<PreparedLocalRelease>;

    async fn current_changelog(&self) -> Result<Option<Changelog>>;
}

/// Prepared artifacts remain under the release source's ownership until this handoff.
#[allow(dead_code)]
pub(crate) enum PreparedLocalRelease {
    #[cfg(windows)]
    VerifiedInstaller(tempfile::TempPath),
    #[cfg(not(windows))]
    Ready,
    NeedsAuthorization,
    Unavailable,
}

impl PreparedLocalRelease {
    pub(super) fn stage(self) -> DownloadReady {
        match self {
            #[cfg(windows)]
            Self::VerifiedInstaller(installer) => {
                super::windows::stage_verified_installer(installer);
                DownloadReady::Yes
            }
            #[cfg(not(windows))]
            Self::Ready => DownloadReady::Yes,
            Self::NeedsAuthorization => DownloadReady::NeedsAuthorization,
            Self::Unavailable => DownloadReady::No,
        }
    }
}
