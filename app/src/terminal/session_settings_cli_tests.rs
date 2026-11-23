use settings::schema::SettingSchemaEntry;
use super::*;
use warpui::{App, SingletonEntity};
use crate::test_util::settings::initialize_settings_for_tests;

#[test]
fn retired_settings_are_inert_and_survive_local_toml_updates() {
    use warpui_extras::user_preferences::UserPreferences;
    use warpui_extras::user_preferences::toml_backed::TomlBackedUserPreferences;

    assert!(
        inventory::iter::<SettingSchemaEntry>
            .into_iter()
            .all(|entry| {
                !entry.hierarchy.is_some_and(|hierarchy| {
                    matches!(hierarchy, "agents.voice" | "tui.voice" | "agents.third_party")
                        || hierarchy.starts_with("agents.third_party.")
                }) && !entry.storage_key.to_ascii_lowercase().contains("voice")
                    && !(entry.hierarchy == Some("agents.warp_agent.input")
                        && entry.storage_key == "agent_toolbar_chip_selection_setting")
            })
    );

    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".agent/tmp");
    std::fs::create_dir_all(&scratch).unwrap();
    let fixture = tempfile::Builder::new()
        .prefix("retired-agent-settings-")
        .tempdir_in(&scratch)
        .unwrap();
    let path = fixture.path().join("settings.toml");
    std::fs::write(&path, r#"
[agents.voice]
voice_input_enabled = "obsolete unregistered value"
voice_input_toggle_key = "alt_right"
[agents.warp_agent.input]
agent_toolbar_chip_selection_setting = { Custom = { left = ["ModelSelector"], right = [] } }
[agents.third_party]
submit_on_ctrl_enter = true
cli_agent_toolbar_chip_selection_setting = { Custom = { left = ["Settings", "FileAttach"], right = [] } }
"#).unwrap();
    let (prefs, error) = TomlBackedUserPreferences::new(path.clone());
    assert!(error.is_none());
    assert_eq!(
        prefs
            .read_value_with_hierarchy("submit_on_ctrl_enter", Some("agents.third_party"))
            .unwrap()
            .as_deref(),
        Some("true")
    );
    let read_legacy_layout = |prefs: &TomlBackedUserPreferences| {
        prefs
            .read_value_with_hierarchy(
                "agent_toolbar_chip_selection_setting",
                Some("agents.warp_agent.input"),
            )
            .unwrap()
            .unwrap()
    };
    let legacy_layout = read_legacy_layout(&prefs);
    assert!(legacy_layout.contains("ModelSelector"));

    let read_layout = |prefs: &TomlBackedUserPreferences| {
        prefs
            .read_value_with_hierarchy(
                "cli_agent_toolbar_chip_selection_setting",
                Some("agents.third_party"),
            )
            .unwrap()
            .unwrap()
    };
    let old_cli_layout = read_layout(&prefs);
    assert!(old_cli_layout.contains("FileAttach"));
    prefs
        .write_value_with_hierarchy(
            "default_tab_config_path",
            "\"local.yaml\"".to_owned(),
            Some("general"),
            None,
        )
        .unwrap();
    let (reloaded, error) = TomlBackedUserPreferences::new(path.clone());
    assert!(error.is_none());
    assert_eq!(read_layout(&reloaded), old_cli_layout);
    assert_eq!(read_legacy_layout(&reloaded), legacy_layout);
    assert_eq!(
        reloaded
            .read_value_with_hierarchy("submit_on_ctrl_enter", Some("agents.third_party"))
            .unwrap()
            .as_deref(),
        Some("true")
    );
    assert_eq!(
        reloaded
            .read_value_with_hierarchy("default_tab_config_path", Some("general"))
            .unwrap()
            .as_deref(),
        Some("\"local.yaml\"")
    );
    let saved = std::fs::read_to_string(path)
        .unwrap()
        .parse::<toml_edit::DocumentMut>()
        .unwrap();
    assert_eq!(
        saved["agents"]["voice"]["voice_input_enabled"].as_str(),
        Some("obsolete unregistered value")
    );
}

#[test]
fn default_tab_settings_keep_old_keys_and_are_local() {
    assert_eq!(DefaultSessionMode::storage_key(), "DefaultSessionMode");
    assert_eq!(DefaultSessionMode::toml_path(), Some("general.default_session_mode"));
    assert!(matches!(DefaultSessionMode::sync_to_cloud(), SyncToCloud::Never));
    assert_eq!(DefaultTabConfigPath::storage_key(), "DefaultTabConfigPath");
    assert_eq!(DefaultTabConfigPath::toml_path(), Some("general.default_tab_config_path"));
    assert!(matches!(DefaultTabConfigPath::sync_to_cloud(), SyncToCloud::Never));
}

#[test]
fn legacy_history_preference_keeps_local_key_and_default() {
    use warpui_extras::user_preferences::UserPreferences;
    use warpui_extras::user_preferences::in_memory::InMemoryPreferences;

    assert!(matches!(IncludeAgentCommandsInHistory::sync_to_cloud(), SyncToCloud::Never));
    assert!(!*IncludeAgentCommandsInHistory::new(None).value());

    App::test((), |mut app| async move {
        let _guard = warp_core::features::FeatureFlag::SettingsFile.override_enabled(false);
        let prefs = InMemoryPreferences::default();
        prefs.write_value("IncludeAgentCommandsInHistory", "true".into()).unwrap();
        app.add_singleton_model(move |_| settings::PrivatePreferences::new(Box::new(prefs)));
        app.add_singleton_model(|_| crate::settings::manager::SettingsManager::default());
        SessionSettings::register(&mut app);
        app.read(|ctx| {
            let session_settings = SessionSettings::as_ref(ctx);
            assert!(*session_settings.include_agent_commands_in_history);
        });
    });
}

#[cfg(windows)]
#[test]
fn windows_warp_ish_runtime_policy_keeps_legacy_preferences_local() {
    const CHILD: &str = "WARP_ISH_RUNTIME_POLICY_TEST";
    if std::env::var_os(CHILD).is_none() {
        // ChannelState is process-global, so isolate the fork identity from other tests.
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "terminal::session_settings::cli_tests::windows_warp_ish_runtime_policy_keeps_legacy_preferences_local",
                "--nocapture",
            ])
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            String::from_utf8_lossy(&output.stdout).contains("warp-ish runtime policy verified")
        );
        return;
    }

    use warp_core::features::FeatureFlag;
    use warp_core::AppId;
    use warp_core::channel::{Channel, ChannelConfig, ChannelState, OzConfig, WarpServerConfig};

    ChannelState::set(ChannelState::new(
        Channel::Oss,
        ChannelConfig {
            app_id: AppId::windows_warp_ish(),
            logfile_name: "".into(),
            server_config: WarpServerConfig::production(),
            oz_config: OzConfig::production(),
            autoupdate_config: None,
            mcp_static_config: None,
        },
    ));
    assert!(ChannelState::is_windows_warp_ish());
    let _docker = FeatureFlag::LocalDockerSandbox.override_enabled(true);

    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        crate::terminal::session_settings::SessionSettings::handle(&app)
            .update(&mut app, |settings, ctx| {
                use crate::terminal::session_settings::DefaultSessionMode;
                for mode in [
                    DefaultSessionMode::Agent,
                    DefaultSessionMode::DockerSandbox,
                ] {
                    settings
                        .default_session_mode_internal
                        .set_value(mode, ctx)
                        .unwrap();
                    assert_eq!(
                        settings.default_session_mode(),
                        DefaultSessionMode::Terminal
                    );
                }
                settings
                    .default_session_mode_internal
                    .set_value(DefaultSessionMode::TabConfig, ctx)
                    .unwrap();
                assert_eq!(
                    settings.default_session_mode(),
                    DefaultSessionMode::TabConfig
                );
            });
        app.update(|ctx| {
            use crate::workspace::header_toolbar_item::HeaderToolbarItemKind;
            assert!(HeaderToolbarItemKind::ToolsPanel.is_supported(ctx));
            assert_eq!(
                HeaderToolbarItemKind::CodeReview.is_supported(ctx),
                cfg!(feature = "local_fs")
            );
        });
    });
    println!("warp-ish runtime policy verified");
}
