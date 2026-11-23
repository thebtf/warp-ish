use serde::{Deserialize, Serialize};
use warpui::{AppContext, Entity, ModelContext, ModelHandle};

#[cfg(feature = "local_fs")]
mod local;
#[cfg(feature = "local_fs")]
pub use local::LocalGitRepoStatusModel;

use super::diff_state::DiffStats;
pub use super::git_repo_models::GitRepoModels;
use crate::context_chips::display_chip::GitBranchTrackingStatus;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CurrentHead {
    BranchName(String),
    HeadlessCommitSha(String),
}

impl CurrentHead {
    pub fn title(&self) -> String {
        match self {
            CurrentHead::BranchName(name) => name.clone(),
            CurrentHead::HeadlessCommitSha(sha) => {
                let short = sha.chars().take(7).collect::<String>();
                format!("Commit {short}")
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DiffBase {
    BranchName(String),
    HeadlessCommitSha(String),
    UncommittedChanges,
}

/// Public metadata exposed to consumers — the subset of diff metadata
/// that the git chip (prompt display, agent view footer) needs.
#[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
#[derive(Debug, Clone)]
pub struct GitStatusMetadata {
    pub current_branch_name: String,
    pub main_branch_name: String,
    pub stats_against_head: DiffStats,
    pub branch_tracking_status: GitBranchTrackingStatus,
}

// ── GitRepoStatusModel ──────────────────────────────────────────────────────

#[derive(Debug)]
pub enum GitRepoStatusEvent {
    /// Emitted whenever the metadata changes (branch name, diff stats, etc.).
    MetadataChanged,
}

/// Per-repository information read from the local repository.
pub struct GitRepoStatusModel {
    #[cfg(feature = "local_fs")]
    inner: ModelHandle<LocalGitRepoStatusModel>,
}

impl Entity for GitRepoStatusModel {
    type Event = GitRepoStatusEvent;
}

#[cfg_attr(not(feature = "local_fs"), allow(dead_code))]
impl GitRepoStatusModel {
    /// Re-emit local repository updates for subscribers.
    fn forward_event(&mut self, event: &GitRepoStatusEvent, ctx: &mut ModelContext<Self>) {
        match event {
            GitRepoStatusEvent::MetadataChanged => ctx.emit(GitRepoStatusEvent::MetadataChanged),
        }
    }

    /// Local status metadata (branch names + HEAD diff stats).
    pub fn metadata<'a>(&self, ctx: &'a AppContext) -> Option<&'a GitStatusMetadata> {
        #[cfg(feature = "local_fs")]
        {
            self.inner.as_ref(ctx).metadata()
        }
        #[cfg(not(feature = "local_fs"))]
        {
            let _ = ctx;
            None
        }
    }

    /// Force a metadata refresh (branch names, diff stats).
    pub fn refresh_metadata(&self, ctx: &mut ModelContext<Self>) {
        #[cfg(feature = "local_fs")]
        {
            self.inner.update(ctx, |m, ctx| m.refresh_metadata(ctx))
        }
        #[cfg(not(feature = "local_fs"))]
        {
            let _ = ctx;
            ()
        }
    }
}

#[cfg(feature = "local_fs")]
pub(super) fn new_local_git_repo_status_model(
    repo_path: std::path::PathBuf,
    repository_model: ModelHandle<repo_metadata::Repository>,
    ctx: &mut ModelContext<GitRepoModels>,
) -> ModelHandle<GitRepoStatusModel> {
    let inner = ctx.add_model(|ctx| LocalGitRepoStatusModel::new(repo_path, repository_model, ctx));
    ctx.add_model(|ctx| {
        ctx.subscribe_to_model(&inner, |me, _, event, ctx| {
            GitRepoStatusModel::forward_event(me, event, ctx)
        });
        GitRepoStatusModel { inner }
    })
}

#[cfg(all(test, feature = "local_fs"))]
impl GitRepoStatusModel {
    /// Wraps a local-backend test model.
    pub(crate) fn new_local_for_test(
        repository: ModelHandle<repo_metadata::Repository>,
        metadata: Option<GitStatusMetadata>,
        ctx: &mut ModelContext<Self>,
    ) -> Self {
        let inner =
            ctx.add_model(move |_| LocalGitRepoStatusModel::new_for_test(repository, metadata));
        ctx.subscribe_to_model(&inner, |me, _, event, ctx| me.forward_event(event, ctx));
        Self { inner }
    }

    pub(crate) fn set_metadata_for_test(
        &mut self,
        metadata: Option<GitStatusMetadata>,
        ctx: &mut ModelContext<Self>,
    ) {
        self.inner.update(ctx, |m, ctx| m.set_metadata_for_test(metadata, ctx))
    }
}
