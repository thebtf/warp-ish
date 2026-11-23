use anyhow::Result;
use channel_versions::Changelog;

use super::ReleaseSource;

pub(crate) async fn get_current_changelog(source: ReleaseSource) -> Result<Option<Changelog>> {
    match source {
        ReleaseSource::Unavailable => Ok(None),
        ReleaseSource::Local(source) => source.current_changelog().await,
    }
}
