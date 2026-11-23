use lsp::LspManagerModel;
use repo_metadata::RepoMetadataModel;
use repo_metadata::repositories::DetectedRepositories;
use repo_metadata::watcher::DirectoryWatcher;
use warp_files::FileModel;
use warpui::{App, SingletonEntity};

use super::GlobalBufferModel;
use crate::test_util::settings::initialize_settings_for_tests;

/// Minimum singletons required by `GlobalBufferModel::new`.
fn init_app(app: &mut App) {
    initialize_settings_for_tests(app);
    app.add_singleton_model(|_| LspManagerModel::new());
    app.add_singleton_model(DirectoryWatcher::new);
    app.add_singleton_model(|_| DetectedRepositories::default());
    app.add_singleton_model(RepoMetadataModel::new);
    app.add_singleton_model(FileModel::new);
}

#[test]
fn removing_a_local_buffer_cleans_up_its_mapping() {
    App::test((), |mut app| async move {
        init_app(&mut app);
        app.add_singleton_model(GlobalBufferModel::new);

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("tracked.txt");
        let model = GlobalBufferModel::handle(&app);
        model.update(&mut app, |model, ctx| {
            let buffer_state = model.open(path.clone(), ctx);
            let file_id = buffer_state.file_id;

            assert_eq!(model.file_path(file_id), Some(path.as_path()));
            model.remove(file_id, ctx);
            assert!(model.file_path(file_id).is_none());
            assert!(!model.buffer_loaded(file_id));
        });
    })
}
