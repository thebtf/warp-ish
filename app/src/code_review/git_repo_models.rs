use std::collections::HashMap;
use std::path::PathBuf;

#[cfg(feature = "local_fs")]
use repo_metadata::repositories::DetectedRepositories;
use warp_util::local_or_remote_path::LocalOrRemotePath;
use warpui::{Entity, ModelContext, ModelHandle, SingletonEntity, WeakModelHandle};

#[cfg(feature = "local_fs")]
use super::git_repo_model::new_local_git_repo_status_model;
use super::git_repo_model::GitRepoStatusModel;
#[cfg(feature = "local_fs")]
use super::github_repo_model::LocalGitHubRepoModel;
use super::github_repo_model::GitHubRepoModel;

// ── GitRepoModels (singleton cache) ─────────────────────────────────────────

/// Singleton model that acts as a cache / factory for per-repository
/// [`GitRepoStatusModel`] and [`GitHubRepoModel`] instances.
///
/// Multiple terminals in the same repo share a single sub-model.  When the last
/// strong handle to a sub-model is dropped, the models are torn down automatically.
pub struct GitRepoModels {
    git_status_models: HashMap<PathBuf, WeakModelHandle<GitRepoStatusModel>>,
    github_repo_models: HashMap<PathBuf, WeakModelHandle<GitHubRepoModel>>,
}
impl GitRepoModels {
    pub fn new() -> Self {
        Self {
            git_status_models: HashMap::new(),
            github_repo_models: HashMap::new(),
        }
    }

    /// Subscribe to local Git status. Callers in the same repository share a model.
    pub fn subscribe(
        &mut self,
        repo: &LocalOrRemotePath,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<ModelHandle<GitRepoStatusModel>> {
        let Some(repo_path) = repo.to_local_path() else {
            anyhow::bail!("Git status is only available for local repositories");
        };
        if let Some(handle) = self
            .git_status_models
            .get(repo_path)
            .and_then(|weak| weak.upgrade(ctx))
        {
            return Ok(handle);
        }

        #[cfg(not(feature = "local_fs"))]
        anyhow::bail!("No watched repository found for path: {}", repo_path.display());

        #[cfg(feature = "local_fs")]
        {
            let Some(repository_model) = DetectedRepositories::as_ref(ctx)
                .get_local_watched_repo_for_path(repo_path, ctx)
            else {
                anyhow::bail!("No watched repository found for path: {}", repo_path.display());
            };
            let handle =
                new_local_git_repo_status_model(repo_path.to_path_buf(), repository_model, ctx);
            self.git_status_models.insert(repo_path.to_path_buf(), handle.downgrade());
            Ok(handle)
        }
    }

    /// Subscribe to GitHub information from the local repository.
    pub fn subscribe_github_repo(
        &mut self,
        repo: &LocalOrRemotePath,
        ctx: &mut ModelContext<Self>,
    ) -> anyhow::Result<ModelHandle<GitHubRepoModel>> {
        let Some(repo_path) = repo.to_local_path() else {
            anyhow::bail!("GitHub information is only available for local repositories");
        };
        if let Some(handle) = self
            .github_repo_models
            .get(repo_path)
            .and_then(|weak| weak.upgrade(ctx))
        {
            return Ok(handle);
        }

        #[cfg(not(feature = "local_fs"))]
        anyhow::bail!(
            "Local GitHub repo info is unavailable without local_fs: {}",
            repo_path.display()
        );

        #[cfg(feature = "local_fs")]
        {
            let git_status = self.subscribe(repo, ctx)?;
            let inner = ctx.add_model(|ctx| {
                LocalGitHubRepoModel::new(repo_path.to_path_buf(), git_status, ctx)
            });
            let handle = ctx.add_model(|ctx| {
                ctx.subscribe_to_model(&inner, |me, _, event, ctx| {
                    GitHubRepoModel::forward_event(me, event, ctx);
                });
                GitHubRepoModel { inner }
            });
            self.github_repo_models.insert(repo_path.to_path_buf(), handle.downgrade());
            Ok(handle)
        }
    }
}

impl Default for GitRepoModels {
    fn default() -> Self {
        Self::new()
    }
}
impl Entity for GitRepoModels {
    type Event = ();
}

impl SingletonEntity for GitRepoModels {}

#[cfg(test)]
mod tests {
    use warp_util::host_id::HostId;
    use warp_util::remote_path::RemotePath;
    use warp_util::standardized_path::StandardizedPath;

    use super::{GitRepoModels, LocalOrRemotePath};

    #[test]
    fn remote_paths_do_not_subscribe_to_local_git_models() {
        warpui::App::test((), |mut app| async move {
            let cache = app.add_model(|_| GitRepoModels::new());
            let remote = LocalOrRemotePath::Remote(RemotePath::new(
                HostId::new("test-host".to_owned()),
                StandardizedPath::try_new("/repository").unwrap(),
            ));
            cache.update(&mut app, |models, ctx| {
                assert!(models.subscribe(&remote, ctx).is_err());
                assert!(models.subscribe_github_repo(&remote, ctx).is_err());
            });
        });
    }
}
