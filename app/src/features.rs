use std::collections::HashSet;

use warp_core::channel::ChannelState;
pub use warp_core::features::*;

/// Mark all features which should be enabled on the current channel as enabled.
/// This sets global feature flag state and should never be called in a unit test.
pub fn init_feature_flags() {
    for flag in enabled_features() {
        flag.set_enabled(true);
    }
    mark_initialized();
}

/// Returns all feature flags which should be enabled in the current channel.
fn enabled_features() -> HashSet<FeatureFlag> {
    // Enable features overridden for the given channel.
    let mut flags = ChannelState::additional_features();

    // Enable flags for release builds, if appropriate.
    if ChannelState::is_release_bundle() {
        flags.extend(RELEASE_FLAGS);
    }

    flags.extend([
        #[cfg(feature = "autoupdate")]
        FeatureFlag::Autoupdate,
        #[cfg(feature = "changelog")]
        FeatureFlag::Changelog,
        #[cfg(feature = "runtime_feature_flags")]
        FeatureFlag::RuntimeFeatureFlags,
        #[cfg(feature = "sequential_storage")]
        FeatureFlag::SequentialStorage,
        #[cfg(feature = "in_band_generators_ssh")]
        FeatureFlag::InBandGeneratorsForSSH,
        #[cfg(feature = "run_generators_with_cmd_exe")]
        FeatureFlag::RunGeneratorsWithCmdExe,
        #[cfg(feature = "ligatures")]
        FeatureFlag::Ligatures,
        #[cfg(feature = "selectable_prompt")]
        FeatureFlag::SelectablePrompt,
        #[cfg(feature = "resize_fix")]
        FeatureFlag::ResizeFix,
        #[cfg(feature = "richtext_multiselect")]
        FeatureFlag::RichTextMultiselect,
        #[cfg(feature = "settings_file")]
        FeatureFlag::SettingsFile,
        #[cfg(feature = "rect_selection")]
        FeatureFlag::RectSelection,
        #[cfg(feature = "alacritty_settings_import")]
        FeatureFlag::AlacrittySettingsImport,
        #[cfg(feature = "dynamic_workflow_enums")]
        FeatureFlag::DynamicWorkflowEnums,
        #[cfg(feature = "shell_selector")]
        FeatureFlag::ShellSelector,
        #[cfg(all(feature = "simulate_github_unauthed", debug_assertions))]
        FeatureFlag::SimulateGithubUnauthed,
        #[cfg(feature = "full_screen_zen_mode")]
        FeatureFlag::FullScreenZenMode,
        #[cfg(feature = "minimalist_ui")]
        FeatureFlag::MinimalistUI,
        #[cfg(feature = "avatar_in_tab_bar")]
        FeatureFlag::AvatarInTabBar,
        #[cfg(feature = "ssh_drag_and_drop")]
        FeatureFlag::SshDragAndDrop,
        #[cfg(feature = "drag_tabs_to_windows")]
        FeatureFlag::DragTabsToWindows,
        #[cfg(feature = "cycle_next_command_suggestion")]
        FeatureFlag::CycleNextCommandSuggestion,
        #[cfg(feature = "multi_workspace")]
        FeatureFlag::MultiWorkspace,
        #[cfg(feature = "ime_marked_text")]
        FeatureFlag::ImeMarkedText,
        #[cfg(feature = "partial_next_command_suggestions")]
        FeatureFlag::PartialNextCommandSuggestions,
        #[cfg(feature = "iterm_images")]
        FeatureFlag::ITermImages,
        #[cfg(feature = "validate_autosuggestions")]
        FeatureFlag::ValidateAutosuggestions,
        #[cfg(feature = "clear_autosuggestion_on_escape")]
        FeatureFlag::ClearAutosuggestionOnEscape,
        #[cfg(feature = "autoupdate_ui_revamp")]
        FeatureFlag::AutoupdateUIRevamp,
        #[cfg(all(not(windows), feature = "kitty_images"))]
        FeatureFlag::KittyImages,
        #[cfg(feature = "warp_packs")]
        FeatureFlag::WarpPacks,
        #[cfg(feature = "default_adeberry_theme")]
        FeatureFlag::DefaultAdeberryTheme,
        #[cfg(feature = "suggested_rules")]
        FeatureFlag::SuggestedRules,
        #[cfg(feature = "command_correction_key")]
        FeatureFlag::CommandCorrectionKey,
        #[cfg(feature = "use_tantivy_search")]
        FeatureFlag::UseTantivySearch,
        #[cfg(feature = "markdown_tables")]
        FeatureFlag::MarkdownTables,
        #[cfg(feature = "jupyter_notebook_rendering")]
        FeatureFlag::JupyterNotebookRendering,
        #[cfg(feature = "blocklist_markdown_table_rendering")]
        FeatureFlag::BlocklistMarkdownTableRendering,
        #[cfg(feature = "blocklist_markdown_images")]
        FeatureFlag::BlocklistMarkdownImages,
        #[cfg(feature = "markdown_mermaid")]
        FeatureFlag::MarkdownMermaid,
        #[cfg(feature = "editable_markdown_mermaid")]
        FeatureFlag::EditableMarkdownMermaid,
        #[cfg(feature = "msys2_shells")]
        FeatureFlag::MSYS2Shells,
        #[cfg(feature = "read_image_files")]
        FeatureFlag::ReadImageFiles,
        #[cfg(feature = "figma_detection")]
        FeatureFlag::FigmaDetection,
        #[cfg(feature = "context_line_review_comments")]
        FeatureFlag::ContextLineReviewComments,
        #[cfg(feature = "code_find_replace")]
        FeatureFlag::CodeFindReplace,
        #[cfg(feature = "command_palette_file_search")]
        FeatureFlag::CommandPaletteFileSearch,
        #[cfg(feature = "expand_edit_to_pane")]
        FeatureFlag::ExpandEditToPane,
        #[cfg(feature = "tab_close_button_on_left")]
        FeatureFlag::TabCloseButtonOnLeft,
        #[cfg(feature = "profiles_design_revamp")]
        FeatureFlag::ProfilesDesignRevamp,
        #[cfg(feature = "search_codebase_ui")]
        FeatureFlag::SearchCodebaseUI,
        #[cfg(feature = "linked_code_blocks")]
        FeatureFlag::LinkedCodeBlocks,
        #[cfg(feature = "tabbed_editor_view")]
        FeatureFlag::TabbedEditorView,
        #[cfg(feature = "undo_closed_panes")]
        FeatureFlag::UndoClosedPanes,
        #[cfg(feature = "multi_profile")]
        FeatureFlag::MultiProfile,
        #[cfg(feature = "get_started_tab")]
        FeatureFlag::GetStartedTab,
        #[cfg(feature = "projects")]
        FeatureFlag::Projects,
        #[cfg(feature = "pr_comments_v2")]
        FeatureFlag::PRCommentsV2,
        #[cfg(feature = "pr_comments_skill")]
        FeatureFlag::PRCommentsSkill,
        #[cfg(feature = "code_mode_chip")]
        FeatureFlag::CodeModeChip,
        #[cfg(feature = "github_pr_prompt_chip")]
        FeatureFlag::GithubPrPromptChip,
        #[cfg(feature = "create_project_flow")]
        FeatureFlag::CreateProjectFlow,
        #[cfg(feature = "vim_code_editor")]
        FeatureFlag::VimCodeEditor,
        #[cfg(feature = "allow_opening_file_links_using_editor_env")]
        FeatureFlag::AllowOpeningFileLinksUsingEditorEnv,
        #[cfg(feature = "revert_diff_hunk")]
        FeatureFlag::RevertDiffHunk,
        #[cfg(feature = "code_review_save_changes")]
        FeatureFlag::CodeReviewSaveChanges,
        #[cfg(feature = "file_tree")]
        FeatureFlag::FileTree,
        #[cfg(feature = "allow_ignoring_input_suggestions")]
        FeatureFlag::AllowIgnoringInputSuggestions,
        #[cfg(feature = "discard_per_file_and_all_changes")]
        FeatureFlag::DiscardPerFileAndAllChanges,
        #[cfg(feature = "code_review_find")]
        FeatureFlag::CodeReviewFind,
        #[cfg(feature = "ui_zoom")]
        FeatureFlag::UIZoom,
        #[cfg(feature = "auto_open_code_review_pane")]
        FeatureFlag::AutoOpenCodeReviewPane,
        #[cfg(feature = "inline_code_review")]
        FeatureFlag::InlineCodeReview,
        #[cfg(feature = "create_environment_slash_command")]
        FeatureFlag::CreateEnvironmentSlashCommand,
        #[cfg(feature = "web_search_ui")]
        FeatureFlag::WebSearchUI,
        #[cfg(feature = "web_fetch_ui")]
        FeatureFlag::WebFetchUI,
        #[cfg(feature = "fork_from_command")]
        FeatureFlag::ForkFromCommand,
        #[cfg(feature = "global_search")]
        FeatureFlag::GlobalSearch,
        #[cfg(feature = "embedded_code_review_comments")]
        FeatureFlag::EmbeddedCodeReviewComments,
        #[cfg(feature = "file_and_diff_set_comments")]
        FeatureFlag::FileAndDiffSetComments,
        #[cfg(feature = "revert_to_checkpoints")]
        FeatureFlag::RevertToCheckpoints,
        #[cfg(feature = "rewind_slash_command")]
        FeatureFlag::RewindSlashCommand,
        #[cfg(feature = "v4a_file_diffs")]
        FeatureFlag::V4AFileDiffs,
        #[cfg(feature = "interactive_conversation_management_view")]
        FeatureFlag::InteractiveConversationManagementView,
        #[cfg(feature = "configurable_toolbar")]
        FeatureFlag::ConfigurableToolbar,
        #[cfg(feature = "classic_completions")]
        FeatureFlag::ClassicCompletions,
        #[cfg(feature = "force_classic_completions")]
        FeatureFlag::ForceClassicCompletions,
        #[cfg(feature = "native_shell_completions")]
        FeatureFlag::NativeShellCompletions,
        #[cfg(feature = "inline_history_menu")]
        FeatureFlag::InlineHistoryMenu,
        #[cfg(feature = "pluggable_notifications")]
        FeatureFlag::PluggableNotifications,
        #[cfg(feature = "async_find")]
        FeatureFlag::AsyncFind,
        #[cfg(feature = "lsp_as_a_tool")]
        FeatureFlag::LSPAsATool,
        #[cfg(feature = "open_warp_launch_modal")]
        FeatureFlag::OpenWarpLaunchModal,
        #[cfg(feature = "new_tab_styling")]
        FeatureFlag::NewTabStyling,
        #[cfg(feature = "incremental_auto_reload")]
        FeatureFlag::IncrementalAutoReload,
        #[cfg(feature = "kitty_keyboard_protocol")]
        FeatureFlag::KittyKeyboardProtocol,
        #[cfg(feature = "inline_menu_headers")]
        FeatureFlag::InlineMenuHeaders,
        #[cfg(feature = "directory_tab_colors")]
        FeatureFlag::DirectoryTabColors,
        #[cfg(feature = "vertical_tabs")]
        FeatureFlag::VerticalTabs,
        #[cfg(feature = "vertical_tabs_summary_mode")]
        FeatureFlag::VerticalTabsSummaryMode,
        #[cfg(feature = "tab_configs")]
        FeatureFlag::TabConfigs,
        #[cfg(feature = "grouped_tabs")]
        FeatureFlag::GroupedTabs,
        #[cfg(feature = "pinned_tabs")]
        FeatureFlag::PinnedTabs,
        #[cfg(feature = "warp_control_cli")]
        FeatureFlag::WarpControlCli,
        #[cfg(feature = "warpify_footer")]
        FeatureFlag::WarpifyFooter,
        #[cfg(feature = "git_operations_in_code_review")]
        FeatureFlag::GitOperationsInCodeReview,
        #[cfg(feature = "trim_trailing_blank_lines")]
        FeatureFlag::TrimTrailingBlankLines,
        #[cfg(feature = "git_credential_refresh")]
        FeatureFlag::GitCredentialRefresh,
        #[cfg(feature = "factory_deferred_repositories")]
        FeatureFlag::FactoryDeferredRepositories,
        #[cfg(feature = "remote_code_review")]
        FeatureFlag::RemoteCodeReview,
        #[cfg(feature = "nld_prompt_history_match")]
        FeatureFlag::NldPromptHistoryMatch,
        #[cfg(feature = "prompt_cache_expiry_warning")]
        FeatureFlag::PromptCacheExpiryWarning,
        #[cfg(feature = "osc_hyperlinks")]
        FeatureFlag::OscHyperlinks,
        #[cfg(feature = "terminal_lifecycle_recovery")]
        FeatureFlag::TerminalLifecycleRecovery,
        #[cfg(feature = "shell_widget_handoff")]
        FeatureFlag::ShellWidgetHandoff,
        #[cfg(feature = "history_search_ranking_v2")]
        FeatureFlag::HistorySearchRankingV2,
        #[cfg(feature = "stored_screenshots")]
        FeatureFlag::StoredScreenshots,
    ]);

    flags
}
