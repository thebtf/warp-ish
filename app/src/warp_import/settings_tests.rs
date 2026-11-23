use super::*;

#[test]
fn projects_schema_keys_and_rejects_incompatible_values() {
    let schema = json!({
        "properties": {
            "general": {"properties": {
                "default_session_mode": {"x-warp-surfaces": ["gui"], "type": "string"},
                "restore_session": {"x-warp-surfaces": ["gui"], "type": "boolean"},
                "mouse_scroll_multiplier": {"x-warp-surfaces": ["gui"], "type": "number"}
            }},
            "appearance": {"properties": {
                "font_size": {"x-warp-surfaces": ["gui"], "type": "number"}
            }}
        }
    });
    let donor = toml::from_str(
        r#"
        [general]
        mouse_scroll_multiplier = 2
        default_session_mode = "tab_config"
        restore_session = false
        [appearance]
        font_size = "large"
        [provider]
        api_key = "donor value"
        token = "another donor value"
    "#,
    )
    .unwrap();
    let mut count = ImportCount::default();
    let selected = project_settings(donor, &schema, &schema, "", &mut count);
    assert_eq!(count.imported, 1);
    assert_eq!(count.skipped, 5);
    assert_eq!(selected["general"].as_table().unwrap().len(), 1);
    assert_eq!(
        selected["general"]["mouse_scroll_multiplier"].as_integer(),
        Some(2)
    );
}

#[test]
fn tab_projection_rejects_platform_panes_cycles_and_network_paths() {
    let tab = r#"
        name = "Split"
        [[panes]]
        id = "root"
        split = "horizontal"
        children = ["first", "second"]
        [[panes]]
        id = "first"
        type = "terminal"
        directory = 'C:\My Project'
        [[panes]]
        id = "second"
        type = "terminal"
        commands = ["Write-Output local"]
    "#;
    let projected: TabConfig = toml::from_str(&project_tab_config(tab).unwrap()).unwrap();
    assert_eq!(
        projected.panes[2].commands.as_deref().unwrap(),
        ["Write-Output local"]
    );
    assert!(project_tab_config(&tab.replace("terminal", "agent")).is_none());
    assert!(project_tab_config(&tab.replace("\"second\"]", "\"root\"]")).is_none());
    assert!(project_tab_config(&tab.replace(r"C:\My Project", r"\\remote\share")).is_none());
    assert!(project_tab_config(&tab.replace(r"C:\My Project", r"C:\file:stream")).is_none());
}

#[test]
fn launch_projection_keeps_legacy_commands_in_explicit_launch_order() {
    let launch = r#"
name: Local
windows:
  - tabs:
      - commands:
          - exec: Write-Output tab
        layout:
          cwd: 'C:\Project'
          commands:
            - exec: Write-Output pane
"#;
    let result = project_launch_config(launch).unwrap();
    assert!(result.find("Write-Output pane").unwrap() < result.find("Write-Output tab").unwrap());
    let parsed: LaunchConfig = serde_yaml::from_str(&result).unwrap();
    assert!(parsed.windows[0].tabs[0].commands.is_empty());
    assert!(
        project_launch_config(&launch.replace(
            "commands:\n            - exec",
            "pane_mode: cloud\n          commands:\n            - exec"
        ))
        .is_none()
    );
}

#[test]
fn projection_writes_only_to_new_destinations_and_never_executes_templates() {
    let temp = tempfile::tempdir().unwrap();
    let donor_root = temp.path().join("donor");
    let local_root = temp.path().join("local");
    let roaming_root = temp.path().join("roaming");
    fs::create_dir_all(donor_root.join("tab_configs")).unwrap();
    fs::create_dir(&local_root).unwrap();
    fs::create_dir(&roaming_root).unwrap();
    let donor = SettingsPaths {
        settings_toml: donor_root.join("settings.toml"),
        data_dir: donor_root.clone(),
        keybindings_yaml: donor_root.join("keybindings.yaml"),
    };
    let staging = SettingsPaths {
        settings_toml: local_root.join("config/settings.toml"),
        data_dir: roaming_root.join("data"),
        keybindings_yaml: local_root.join("config/keybindings.yaml"),
    };
    let sentinel = temp.path().join("must-not-exist");
    let command = format!(
        "New-Item -ItemType File -LiteralPath '{}'",
        sentinel.display()
    );
    // A TOML basic string safely includes PowerShell's single quotes and escaped path.
    let tab = format!(
        "name = 'Local'\n[[panes]]\nid = 'main'\ntype = 'terminal'\ncommands = [{}]\n",
        serde_json::to_string(&command).unwrap(),
    );
    fs::write(donor_root.join("tab_configs/local.toml"), &tab).unwrap();
    fs::write(&donor.settings_toml, "[terminal]\ncopy_on_select = false\n").unwrap();
    fs::write(
        &donor.keybindings_yaml,
        "'terminal:copy': 'ctrl-c'\n'platform:sign_in': none\n",
    )
    .unwrap();
    let roots = [local_root.as_path(), roaming_root.as_path()];
    let report = project_local_files(&donor, &staging, &roots).unwrap();
    assert_eq!(
        (report.keybindings.imported, report.keybindings.skipped),
        (1, 1)
    );
    assert_eq!(report.tab_configs.imported, 1);
    assert_eq!(report.settings_keys.imported, 1);
    assert!(!sentinel.exists());
    let settings: Table =
        toml::from_str(&fs::read_to_string(&staging.settings_toml).unwrap()).unwrap();
    assert_eq!(settings["general"]["restore_session"].as_bool(), Some(true));
    assert_eq!(
        settings["terminal"]["copy_on_select"].as_bool(),
        Some(false)
    );
    assert!(project_local_files(&donor, &staging, &roots).is_err());
    assert!(project_local_files(&donor, &staging, &[&roaming_root]).is_err());
    assert_eq!(
        fs::read_to_string(donor_root.join("tab_configs/local.toml")).unwrap(),
        tab
    );
}
