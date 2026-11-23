use warpui::EntityId;

use super::WorkspaceAction;
use crate::pane_group::TerminalPaneId;
use crate::workspace::PaneViewLocator;
use crate::workspace::tab_settings::{
    VerticalTabsDisplayGranularity, VerticalTabsPrimaryInfo, VerticalTabsTabItemMode,
    VerticalTabsViewMode,
};

#[test]
fn vertical_tabs_view_mode_change_does_not_save_workspace_state() {
    assert!(
        !WorkspaceAction::SetVerticalTabsViewMode(VerticalTabsViewMode::Compact)
            .should_save_app_state_on_action()
    );
}

#[test]
fn vertical_tabs_panel_toggle_still_saves_workspace_state() {
    assert!(WorkspaceAction::ToggleVerticalTabsPanel.should_save_app_state_on_action());
}

#[test]
fn settings_popup_toggle_does_not_save_workspace_state() {
    assert!(!WorkspaceAction::ToggleVerticalTabsSettingsPopup.should_save_app_state_on_action());
}

#[test]
fn display_granularity_change_does_not_save_workspace_state() {
    assert!(
        !WorkspaceAction::SetVerticalTabsDisplayGranularity(VerticalTabsDisplayGranularity::Panes)
            .should_save_app_state_on_action()
    );
    assert!(
        !WorkspaceAction::SetVerticalTabsDisplayGranularity(VerticalTabsDisplayGranularity::Tabs)
            .should_save_app_state_on_action()
    );
}

#[test]
fn tab_item_mode_change_does_not_save_workspace_state() {
    assert!(
        !WorkspaceAction::SetVerticalTabsTabItemMode(VerticalTabsTabItemMode::FocusedSession)
            .should_save_app_state_on_action()
    );
    assert!(
        !WorkspaceAction::SetVerticalTabsTabItemMode(VerticalTabsTabItemMode::Summary)
            .should_save_app_state_on_action()
    );
}

#[test]
fn primary_info_change_does_not_save_workspace_state() {
    assert!(
        !WorkspaceAction::SetVerticalTabsPrimaryInfo(VerticalTabsPrimaryInfo::Command)
            .should_save_app_state_on_action()
    );
    assert!(
        !WorkspaceAction::SetVerticalTabsPrimaryInfo(VerticalTabsPrimaryInfo::WorkingDirectory)
            .should_save_app_state_on_action()
    );
    assert!(
        !WorkspaceAction::SetVerticalTabsPrimaryInfo(VerticalTabsPrimaryInfo::Branch)
            .should_save_app_state_on_action()
    );
}

#[test]
fn pane_name_actions_save_workspace_state() {
    let locator = PaneViewLocator {
        pane_group_id: EntityId::new(),
        pane_id: TerminalPaneId::dummy_terminal_pane_id().into(),
    };

    assert!(WorkspaceAction::RenamePane(locator).should_save_app_state_on_action());
    assert!(WorkspaceAction::ResetPaneName(locator).should_save_app_state_on_action());
    // GH-9351: the keyboard-bindable variant must persist app state on the
    // same conditions as the locator-based one, since both ultimately drive
    // `rename_pane` which mutates `pane_configuration`.
    assert!(WorkspaceAction::RenameActivePane.should_save_app_state_on_action());
}

#[test]
fn terminal_product_actions_keep_local_tools_and_reject_builtin_platform() {
    use crate::palette::PaletteMode;
    use super::PaletteSource;
    use crate::settings_view::SettingsSection;

    for action in [
        WorkspaceAction::AddTerminalTab {
            hide_homepage: false,
        },
        WorkspaceAction::NewCodeFile,
        WorkspaceAction::OpenRepository { path: None },
        WorkspaceAction::ToggleRightPanel,
        WorkspaceAction::RunCommand("omp".to_owned()),
        WorkspaceAction::ShowSettingsPage(SettingsSection::EditorAndCodeReview),
        WorkspaceAction::ToggleLeftPanel,
        WorkspaceAction::ToggleProjectExplorer,
        WorkspaceAction::TogglePalette {
            mode: PaletteMode::Files,
            source: PaletteSource::Keybinding,
        },
    ] {
        assert!(action.is_terminal_action(), "{action:?}");
    }
    for action in [
        WorkspaceAction::ShowSettingsPage(SettingsSection::Account),
    ] {
        assert!(!action.is_terminal_action(), "{action:?}");
    }
}

#[test]
fn terminal_product_actions_keep_file_notebook_and_local_workflow_commands() {
    use crate::notebooks::NotebookLocation;
    use crate::workflows::workflow::Workflow;
    use crate::workflows::{WorkflowSelectionSource, WorkflowSource, WorkflowType};
    use std::sync::Arc;

    let workflow = Workflow::new("Local command", "echo hello");
    let action = |workflow, workflow_source| WorkspaceAction::RunWorkflow {
        workflow: Arc::new(workflow),
        workflow_source,
        workflow_selection_source: WorkflowSelectionSource::WorkflowView,
        argument_override: None,
    };
    assert!(
        action(
            WorkflowType::Local(workflow.clone()),
            WorkflowSource::Project
        )
        .is_terminal_action()
    );
    for location in [NotebookLocation::LocalFile, NotebookLocation::RemoteFile] {
        assert!(
            action(
                WorkflowType::Notebook(workflow.clone()),
                WorkflowSource::Notebook {
                    notebook_id: None,
                    team_uid: None,
                    location,
                }
            )
            .is_terminal_action()
        );
    }
    assert!(
        !action(
            WorkflowType::Notebook(workflow.clone()),
            WorkflowSource::Notebook {
                notebook_id: None,
                team_uid: None,
                location: NotebookLocation::PersonalCloud,
            }
        )
        .is_terminal_action()
    );

}

#[cfg(debug_assertions)]
#[test]
fn terminal_product_actions_reject_platform_debug_controls() {
    for action in [
        WorkspaceAction::InstallOpenCodeWarpPlugin,
        WorkspaceAction::UseLocalOpenCodeWarpPlugin,
        WorkspaceAction::ShowHoaOnboardingFlow,
    ] {
        assert!(!action.is_terminal_action(), "{action:?}");
    }
    assert!(WorkspaceAction::ShowSessionConfigModal.is_terminal_action());
    assert!(WorkspaceAction::OpenViewTreeDebugWindow.is_terminal_action());
}
