use std::collections::HashSet;

use serde_json::json;
use settings::SettingsMode;
use settings::schema::SettingSchemaEntry;

use super::{
    product_policy, setting_surface_names, settings_schema_json_for_product,
    strip_empty_enum_entries, strip_numeric_metadata,
};

#[test]
fn strips_numeric_metadata_recursively() {
    let mut schema = json!({
        "type": "object",
        "properties": {
            "count": {
                "type": "integer",
                "minimum": 0,
                "maximum": 255,
                "format": "uint8"
            }
        }
    });

    strip_numeric_metadata(&mut schema);

    assert_eq!(
        schema,
        json!({
            "type": "object",
            "properties": {
                "count": {
                    "type": "integer"
                }
            }
        })
    );
}

#[test]
fn strips_empty_enum_entries() {
    let mut schema = json!({
        "oneOf": [
            {
                "enum": [],
                "type": "string"
            },
            {
                "const": "kept"
            }
        ]
    });

    strip_empty_enum_entries(&mut schema);

    assert_eq!(
        schema,
        json!({
            "oneOf": [
                {
                    "const": "kept"
                }
            ]
        })
    );
}

#[test]
fn generates_a_settings_schema() {
    let schema = settings_schema_json_for_product(|_| false, false).unwrap();
    let schema: serde_json::Value = serde_json::from_str(&schema).unwrap();

    assert_eq!(schema["title"], "Warp Settings");
    assert_eq!(schema["type"], "object");
    assert!(schema["properties"].is_object());
}

#[test]
fn surface_annotation_matches_setting_schema_entry_metadata() {
    for entry in inventory::iter::<SettingSchemaEntry> {
        let surfaces = (entry.surfaces_fn)();
        let annotation = setting_surface_names(surfaces);
        let annotation_names: HashSet<&str> = annotation
            .iter()
            .filter_map(|value| value.as_str())
            .collect();

        assert_eq!(
            annotation_names.contains("gui"),
            surfaces.includes(SettingsMode::Gui),
            "GUI surface mismatch for {}",
            entry.storage_key
        );
        assert_eq!(
            annotation_names.contains("tui"),
            surfaces.includes(SettingsMode::Tui),
            "TUI surface mismatch for {}",
            entry.storage_key
        );
        assert_eq!(
            annotation_names.len(),
            usize::from(surfaces.includes(SettingsMode::Gui))
                + usize::from(surfaces.includes(SettingsMode::Tui)),
            "unexpected surface annotation for {}",
            entry.storage_key
        );
    }
}

#[cfg(windows)]
#[test]
fn terminal_schema_retains_exact_public_product_inventory() {
    let schema: serde_json::Value =
        serde_json::from_str(&settings_schema_json_for_product(|_| true, true).unwrap()).unwrap();
    assert_eq!(schema["title"], "warp-ish Settings");
    assert_eq!(schema["additionalProperties"], false);
    let expected: HashSet<_> = product_policy::TERMINAL_PUBLIC_SETTINGS
        .iter()
        .flat_map(|(group, keys)| keys.iter().map(move |key| format!("{group}.{key}")))
        .collect();
    assert_eq!(expected.len(), 129);
    let actual: HashSet<_> = inventory::iter::<SettingSchemaEntry>
        .into_iter()
        .filter_map(|entry| {
            let group = entry.hierarchy?;
            let pointer = format!(
                "/properties/{}/properties/{}",
                group.replace('.', "/properties/"),
                entry.storage_key
            );
            schema
                .pointer(&pointer)
                .map(|_| format!("{group}.{}", entry.storage_key))
        })
        .collect();
    assert_eq!(actual, expected);

    for path in [
        "code.editor.prefer_markdown_viewer",
        "code.editor.open_file_editor",
        "code.editor.show_code_review_button",
        "code.editor.show_code_review_diff_stats",
        "appearance.text.notebook_font_size",
        "general.default_tab_config_path",
        "privacy.custom_secret_regex_list",
        "session.working_directory_config",
    ] {
        assert!(actual.contains(path), "missing local setting: {path}");
    }
    for path in [
        "agents.warp_agent.is_any_ai_enabled",
        "agents.mcp_servers.file_based_mcp_enabled",
        "cloud_platform.third_party_api_keys.aws_bedrock_auto_login",
        "account.is_settings_sync_enabled",
        "notifications.toast_duration_secs",
        "privacy.telemetry_enabled",
        "privacy.crash_reporting_enabled",
        "terminal.input.at_context_menu_in_terminal_mode",
        "terminal.show_terminal_zero_state_block",
        "code.editor.auto_open_code_review_pane_on_first_agent_change",
        "warpify.ssh.ssh_extension_install_mode",
    ] {
        assert!(
            !actual.contains(path),
            "unexpected platform setting: {path}"
        );
    }
}

#[cfg(windows)]
#[test]
fn terminal_schema_validates_local_defaults_and_rejects_platform_values() {
    let schema: serde_json::Value =
        serde_json::from_str(&settings_schema_json_for_product(|_| true, true).unwrap()).unwrap();
    let validator = jsonschema::draft202012::new(&schema).unwrap();
    let mut defaults = json!({});
    for entry in inventory::iter::<SettingSchemaEntry> {
        let Some(group) = entry.hierarchy else {
            continue;
        };
        let pointer = format!(
            "/properties/{}/properties/{}",
            group.replace('.', "/properties/"),
            entry.storage_key
        );
        let Some(default) = schema
            .pointer(&pointer)
            .and_then(|leaf| leaf.get("default"))
        else {
            continue;
        };
        let mut parent = &mut defaults;
        for segment in group.split('.') {
            parent = parent
                .as_object_mut()
                .unwrap()
                .entry(segment)
                .or_insert_with(|| json!({}));
        }
        parent
            .as_object_mut()
            .unwrap()
            .insert(entry.storage_key.to_string(), default.clone());
    }
    assert!(validator.is_valid(&defaults));
    assert!(validator.is_valid(&json!({"general": {"default_session_mode": "tab_config", "default_tab_config_path": "local-tab.yaml"}})));
    assert!(
        validator.is_valid(&json!({"terminal": {"input": {"input_box_type_setting": "classic"}}}))
    );
    assert!(validator.is_valid(&json!({"appearance": {"tabs": {"header_toolbar_chip_selection": {"custom": {"left": ["tabs_panel", "tools_panel"], "right": ["code_review"]}}}}})));

    // Keep the original toolbar's acceptance of compatible extra Custom fields.
    assert!(validator.is_valid(&json!({"appearance": {"tabs": {"header_toolbar_chip_selection": {"custom": {"left": ["tabs_panel"], "right": [], "future_local_field": true}}}}})));
    // NotificationsSettings uses serde(default): an omitted field keeps its default.
    assert!(validator.is_valid(&json!({"notifications": {"preferences": {}}})));
    assert!(
        validator.is_valid(
            &json!({"notifications": {"preferences": {"is_long_running_enabled": false}}})
        )
    );
    let properties = schema
        .pointer("/$defs/TerminalNotificationPreferences/properties")
        .unwrap()
        .as_object()
        .unwrap();
    let defaults_object = defaults["notifications"]["preferences"]
        .as_object()
        .unwrap();
    for (key, value) in defaults_object {
        assert_eq!(
            properties[key]["default"], *value,
            "wrong nested default for {key}"
        );
    }

    for value in ["agent", "cloud_agent", "docker_sandbox"] {
        assert!(!validator.is_valid(&json!({"general": {"default_session_mode": value}})));
    }
    // Universal is the inherited ordinary Warp prompt, also used with AI disabled.
    assert!(
        validator
            .is_valid(&json!({"terminal": {"input": {"input_box_type_setting": "universal"}}}))
    );
    assert!(
        !validator.is_valid(&json!({"agents": {"mcp_servers": {"file_based_mcp_enabled": true}}}))
    );
    assert!(!validator.is_valid(&json!({"privacy": {"telemetry_enabled": true}})));
    assert!(!validator.is_valid(&json!({"general": {"unknown_platform_option": true}})));
    for item in ["agent_management", "notifications_mailbox"] {
        assert!(!validator.is_valid(&json!({"appearance": {"tabs": {"header_toolbar_chip_selection": {"custom": {"left": [item], "right": []}}}}})));
    }

    let notifications = &mut defaults["notifications"]["preferences"];
    assert!(notifications.get("is_long_running_enabled").is_some());
    assert!(
        notifications
            .get("is_agent_task_completed_enabled")
            .is_none()
    );
    notifications
        .as_object_mut()
        .unwrap()
        .insert("is_agent_task_completed_enabled".into(), json!(true));
    assert!(!validator.is_valid(&defaults));
}

#[test]
fn unfiltered_schema_excludes_retired_settings_and_keeps_legacy_values() {
    let schema: serde_json::Value =
        serde_json::from_str(&settings_schema_json_for_product(|_| true, false).unwrap()).unwrap();
    assert!(
        schema
            .pointer("/properties/agents/properties/mcp_servers/properties/file_based_mcp_enabled")
            .is_none()
    );
    assert!(
        schema
            .pointer("/properties/account/properties/is_settings_sync_enabled")
            .is_none()
    );
    for key in [
        "at_context_menu_in_terminal_mode",
        "enable_slash_commands_in_terminal",
        "enable_ai_command_search_hash_trigger",
        "outline_codebase_symbols_for_at_context_menu",
    ] {
        let pointer = format!("/properties/terminal/properties/input/properties/{key}");
        assert!(schema.pointer(&pointer).is_none(), "unexpected input setting: {key}");
    }
    let validator = jsonschema::draft202012::new(&schema).unwrap();
    assert!(validator.is_valid(&json!({"general": {"default_session_mode": "agent"}})));
    assert!(
        validator
            .is_valid(&json!({"terminal": {"input": {"input_box_type_setting": "universal"}}}))
    );
}
