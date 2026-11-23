//! Public settings belonging to the Windows local terminal product.
//!
//! Match the TOML hierarchy and key because some settings groups contain
//! both terminal and built-in agent settings. This policy applies to public
//! preferences and can also be used when they are read from storage.

pub(crate) const TERMINAL_PUBLIC_SETTINGS: &[(&str, &[&str])] = &[
    ("accessibility", &["accessibility_verbosity"]),
    ("appearance", &["spacing"]),
    (
        "appearance.blocks",
        &[
            "should_show_bootstrap_block",
            "should_show_in_band_command_blocks",
            "should_show_ssh_block",
            "show_jump_to_bottom_of_block_button",
            "show_block_dividers",
        ],
    ),
    (
        "appearance.cursor",
        &["cursor_blink", "cursor_display_type"],
    ),
    ("appearance.full_screen_apps", &["alt_screen_padding"]),
    ("appearance.input", &["input_mode"]),
    (
        "appearance.panes",
        &["should_dim_inactive_panes", "focus_pane_on_hover"],
    ),
    (
        "appearance.tabs",
        &[
            "tab_close_button_position",
            "workspace_decoration_visibility",
            "directory_tab_colors",
            "header_toolbar_chip_selection",
            "show_indicators_button",
            "preserve_active_tab_color",
        ],
    ),
    (
        "appearance.text",
        &[
            "font_name",
            "font_size",
            "font_weight",
            "line_height_ratio",
            "notebook_font_size",
            "match_notebook_to_monospace_font_size",
            "enforce_minimum_contrast",
            "ligature_rendering_enabled",
        ],
    ),
    (
        "appearance.themes",
        &["theme", "system_theme", "selected_system_themes"],
    ),
    (
        "appearance.vertical_tabs",
        &[
            "view_mode",
            "display_granularity",
            "tab_item_mode",
            "primary_info",
            "compact_subtitle",
            "enabled",
            "show_panel_in_restored_windows",
            "hide_title_bar_search_bar",
            "show_pr_link",
            "show_diff_stats",
            "show_details_on_hover",
        ],
    ),
    (
        "appearance.window",
        &[
            "backdrop",
            "override_blur_texture",
            "override_opacity",
            "open_windows_at_custom_size",
            "new_windows_num_columns",
            "new_windows_num_rows",
            "left_panel_visibility_across_tabs",
            "zoom_level",
        ],
    ),
    (
        "code.editor",
        &[
            "use_warp_as_default_editor",
            "show_project_explorer",
            "show_global_search",
            "show_hidden_files",
            "format_on_save",
            "auto_save",
            "show_code_review_button",
            "show_code_review_diff_stats",
            "open_file_editor",
            "open_code_panels_file_editor",
            "open_file_layout",
            "prefer_markdown_viewer",
            "prefer_tabbed_editor_view",
        ],
    ),
    ("experimental", &["async_find_enabled"]),
    (
        "general",
        &[
            "show_warning_before_quitting",
            "restore_session",
            "login_item",
            "link_tooltip",
            "should_confirm_close_session",
            "mouse_scroll_multiplier",
            "new_tab_placement",
            "preserve_input_focus_on_block_selection",
            "snackbar_enabled",
            "default_session_mode",
            "default_tab_config_path",
        ],
    ),
    ("general.undo_close", &["enabled", "grace_period"]),
    ("global_hotkey.dedicated_window", &["settings", "enabled"]),
    (
        "global_hotkey.toggle_all_windows",
        &["enabled", "keybinding"],
    ),
    ("keys", &["ctrl_tab_behavior_setting"]),
    ("notifications", &["preferences"]),
    ("privacy", &["custom_secret_regex_list"]),
    (
        "privacy.secret_redaction",
        &[
            "enabled",
            "secret_display_mode_setting",
            "hide_secrets_in_block_list",
        ],
    ),
    (
        "session",
        &[
            "startup_shell_override",
            "new_session_shell_override",
            "working_directory_config",
        ],
    ),
    (
        "system",
        &["prefer_low_power_gpu", "preferred_graphics_backend"],
    ),
    (
        "terminal",
        &[
            "copy_on_select",
            "mouse_reporting_enabled",
            "scroll_reporting_enabled",
            "focus_reporting_enabled",
            "use_audible_bell",
            "maximum_grid_size",
            "osc52_clipboard_access",
        ],
    ),
    (
        "terminal.input",
        &[
            "alias_expansion_enabled",
            "honor_ps1",
            "extra_meta_keys",
            "show_hint_text",
            "classic_completions_mode",
            "completions_open_while_typing",
            "warp_completions_enabled",
            "native_shell_completions_enabled",
            "error_underlining_enabled",
            "syntax_highlighting",
            "command_corrections",
            "input_box_type_setting",
            "middle_click_paste_enabled",
            "right_click_behavior",
        ],
    ),
    (
        "terminal.input.autosuggestions",
        &["enabled", "keybinding_hint", "show_ignore_button"],
    ),
    ("terminal.smart_select", &["enabled", "word_char_allowlist"]),
    (
        "text_editing",
        &[
            "vim_mode_enabled",
            "vim_unnamed_system_clipboard",
            "vim_status_bar",
            "code_editor_line_number_mode",
            "autocomplete_symbols",
        ],
    ),
    (
        "warpify.subshells",
        &["added_subshell_commands", "subshell_commands_denylist"],
    ),
    (
        "warpify.ssh",
        &[
            "reuse_existing_control_master",
            "ssh_hosts_denylist",
            "enable_ssh_warpification",
            "enable_legacy_ssh_wrapper",
        ],
    ),
    ("workflows", &["show_global_workflows_in_universal_search"]),
];

pub(crate) fn is_terminal_public_setting(hierarchy: Option<&str>, key: &str) -> bool {
    TERMINAL_PUBLIC_SETTINGS
        .iter()
        .any(|(group, keys)| Some(*group) == hierarchy && keys.contains(&key))
}
