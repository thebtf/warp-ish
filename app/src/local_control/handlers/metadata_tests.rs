use super::{SurfaceDestination, surface_unavailable_reason};
use crate::features::FeatureFlag;

#[test]
fn vertical_tabs_surface_reports_feature_flag_unavailable() {
    let flag_guard = FeatureFlag::VerticalTabs.override_enabled(false);
    warpui::App::test((), |mut app| async move {
        assert_eq!(
            app.update(|ctx| { surface_unavailable_reason(SurfaceDestination::VerticalTabs, ctx) }),
            Some("vertical tabs are unavailable or disabled")
        );
    });
    drop(flag_guard);
}

#[test]
fn mcp_location_matches_the_terminal_uuid_across_windows() {
    use ::local_control::{ErrorCode, InstanceId};

    use super::where_am_i;
    use crate::app_state::{LeafContents, PaneNodeSnapshot};
    use crate::workspace::view::tests::{initialize_app, mock_workspace};

    warpui::App::test((), |mut app| async move {
        initialize_app(&mut app);
        let first = mock_workspace(&mut app);
        let second = mock_workspace(&mut app);
        let instance = InstanceId("inst_test".into());
        app.read(|ctx| {
            assert_ne!(first.window_id(ctx), second.window_id(ctx));
            let tab = first.as_ref(ctx).active_tab_pane_group();
            let pane_group = tab.as_ref(ctx);
            let PaneNodeSnapshot::Leaf(pane) = pane_group.snapshot(ctx) else {
                panic!("expected one terminal pane");
            };
            let LeafContents::Terminal(terminal) = pane.contents else {
                panic!("expected a terminal pane");
            };
            let uuid: String = terminal
                .uuid
                .iter()
                .map(|byte| format!("{byte:02X}"))
                .collect();
            let response = where_am_i(&instance, &uuid, Some("wrong block"), ctx).unwrap();
            let pane_id = pane_group
                .find_terminal_pane_by_session_uuid(&terminal.uuid)
                .unwrap();
            assert_eq!(response["window_id"], first.window_id(ctx).to_string());
            assert_eq!(response["tab_id"], tab.id().to_string());
            assert_eq!(response["pane_id"], pane_id.to_string());
            assert_eq!(response["instance_id"], "inst_test");
            assert_eq!(response["terminal_uuid"], uuid.to_ascii_lowercase());
            assert!(response["tab_group_id"].is_null());
            assert!(response["block_id"].is_null());
            assert_eq!(
                where_am_i(&instance, &"0".repeat(32), None, ctx)
                    .unwrap_err()
                    .code,
                ErrorCode::StaleTarget,
            );
            assert_eq!(
                where_am_i(&instance, "g", None, ctx).unwrap_err().code,
                ErrorCode::InvalidParams,
            );
        });
    });
}
