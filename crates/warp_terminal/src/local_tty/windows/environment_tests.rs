use super::*;

#[test]
fn wsl_env_allowlist_includes_client_version() {
    let wslenv = wsl_env_allowlist(false).to_string_lossy().into_owned();

    assert_eq!(
        wslenv.split(':').collect::<Vec<_>>(),
        vec![
            format!("{HONOR_PS1_NAME}/u"),
            format!("{USE_SSH_WRAPPER_NAME}/u"),
            format!("{SSH_REUSE_CONTROL_MASTER_NAME}/u"),
            format!("{SHELL_DEBUG_MODE_NAME}/u"),
            format!("{TERM_PROGRAM_NAME}/u"),
            format!("{IS_LOCAL_SESSION_NAME}/u"),
            format!("{SSH_SOCKET_DIR}/u"),
            format!("{WARP_CLIENT_VERSION_ENV}/u"),
            format!("{TERMINAL_SESSION_UUID_ENV}/u"),
            format!("{FOCUS_URL_ENV}/u"),
            format!("{PROMPT_NODE_VERSION_ENABLED_NAME}/u"),
        ],
    );
}

#[test]
fn wsl_env_allowlist_includes_initial_working_directory_when_requested() {
    let wslenv = wsl_env_allowlist(true).to_string_lossy().into_owned();

    assert_eq!(
        wslenv.split(':').collect::<Vec<_>>(),
        vec![
            format!("{HONOR_PS1_NAME}/u"),
            format!("{USE_SSH_WRAPPER_NAME}/u"),
            format!("{SSH_REUSE_CONTROL_MASTER_NAME}/u"),
            format!("{SHELL_DEBUG_MODE_NAME}/u"),
            format!("{TERM_PROGRAM_NAME}/u"),
            format!("{IS_LOCAL_SESSION_NAME}/u"),
            format!("{SSH_SOCKET_DIR}/u"),
            format!("{WARP_CLIENT_VERSION_ENV}/u"),
            format!("{TERMINAL_SESSION_UUID_ENV}/u"),
            format!("{FOCUS_URL_ENV}/u"),
            format!("{PROMPT_NODE_VERSION_ENABLED_NAME}/u"),
            format!("{INITIAL_WORKING_DIR_NAME}/pu"),
        ],
    );
}

#[test]
fn ordinary_windows_shell_drops_inherited_fixture_history_marker() {
    let mut env = BTreeMap::from([(
        map_key("warp_ish_fixture_psreadline_history".into()),
        EnvEntry {
            preferred_key: "warp_ish_fixture_psreadline_history".into(),
            value: r"C:\other\history.txt".into(),
        },
    )]);

    apply_powershell_fixture_history(&mut env, None);

    assert!(!env.contains_key(&map_key(FIXTURE_PSREADLINE_HISTORY.into())));
}

#[test]
fn windows_powershell_fixture_uses_history_path_owned_by_launcher() {
    let mut env = BTreeMap::from([(
        map_key("warp_ish_fixture_psreadline_history".into()),
        EnvEntry {
            preferred_key: "warp_ish_fixture_psreadline_history".into(),
            value: r"C:\other\history.txt".into(),
        },
    )]);
    let fixture_history = PathBuf::from(r"C:\fixture\psreadline\ConsoleHost_history.txt");

    apply_powershell_fixture_history(&mut env, Some(fixture_history.clone()));

    let history = &env[&map_key(FIXTURE_PSREADLINE_HISTORY.into())];
    assert_eq!(
        history.preferred_key,
        OsString::from(FIXTURE_PSREADLINE_HISTORY)
    );
    assert_eq!(history.value, fixture_history.into_os_string());
    assert_eq!(
        env[&map_key(IS_LOCAL_SESSION_NAME.into())].value,
        OsString::from("1"),
    );
}
