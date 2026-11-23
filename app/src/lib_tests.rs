use super::*;

#[test]
fn app_keeps_default_secure_storage_service_name() {
    let launch_mode = LaunchMode::App {
        args: Default::default(),
    };

    assert_eq!(
        launch_mode.secure_storage_service_name("dev.warp.Warp-Dev"),
        "dev.warp.Warp-Dev"
    );
}

#[test]
fn launch_modes_select_expected_logging_frontend() {
    let app = LaunchMode::App {
        args: Default::default(),
    };
    let test = LaunchMode::Test {
        driver: Box::new(None),
        is_integration_test: false,
    };

    assert_eq!(app.log_frontend(), LogFrontend::Gui);
    assert_eq!(test.log_frontend(), LogFrontend::Gui);
}

#[test]
fn only_fork_gui_disables_mcp_startup_and_watch_registration() {
    let app = LaunchMode::App {
        args: Default::default(),
    };
    assert!(app.is_local_terminal_gui(true));
    assert!(!app.is_local_terminal_gui(false));

    let test = LaunchMode::Test {
        driver: Box::new(None),
        is_integration_test: false,
    };
    assert!(!test.is_local_terminal_gui(true));
    assert!(!test.is_local_terminal_gui(false));
}

#[cfg(enable_crash_recovery)]
#[test]
fn local_startup_recovery_remains_available_without_crash_reporting() {
    let app = LaunchMode::App {
        args: Default::default(),
    };
    assert!(app.crash_recovery_enabled());
}
