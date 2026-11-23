#[cfg(windows)]
use warp_core::features::FeatureFlag;
#[cfg(windows)]
use warpui::platform::WindowStyle;
use warpui::App;

use super::NewWorkspaceSource;
#[cfg(windows)]
use super::RootView;
use crate::GlobalResourceHandles;

#[cfg(windows)]
#[test]
fn windows_root_opens_workspace_and_accepts_a_local_session() {
    let _force_login = FeatureFlag::ForceLogin.override_enabled(true);

    App::test((), |mut app| async move {
        crate::workspace::view::tests::initialize_app(&mut app);
        let global_resource_handles = GlobalResourceHandles::mock(&mut app);
        let (_, root_view) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
            RootView::new(
                global_resource_handles,
                NewWorkspaceSource::Empty {
                    previous_active_window: None,
                    shell: None,
                },
                ctx,
            )
        });

        root_view.update(&mut app, |root, ctx| {
            let workspace = root.workspace_view().expect("local workspace").clone();
            let initial_tabs = workspace.as_ref(ctx).tab_count();
            assert!(initial_tabs > 0);

            let path = std::env::current_dir().expect("fixture working directory");
            assert!(root.add_session_at_path(&path, ctx));
            assert_eq!(root.workspace_view().expect("local workspace").id(), workspace.id());
            assert_eq!(workspace.as_ref(ctx).tab_count(), initial_tabs + 1);
        });
    });
}
