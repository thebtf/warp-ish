use std::ffi::OsString;

use super::*;
use crate::agent::Harness;

#[test]
fn identifies_worker_subcommands() {
    assert!(!is_worker_invocation("minidump-server"));
    #[cfg(unix)]
    assert!(is_worker_invocation(&terminal_server_subcommand()));
    assert!(!is_worker_invocation("--prompt"));
}

fn set_env_var(name: &str, value: &str) -> Option<OsString> {
    let previous = std::env::var_os(name);
    // Safety: tests that mutate process environment are marked `serial` so we
    // do not race with other environment readers/writers in this crate.
    unsafe { std::env::set_var(name, value) };
    previous
}

fn restore_env_var(name: &str, previous: Option<OsString>) {
    match previous {
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        Some(value) => unsafe { std::env::set_var(name, value) },
        // Safety: tests that mutate process environment are marked `serial` so
        // we do not race with other environment readers/writers in this crate.
        None => unsafe { std::env::remove_var(name) },
    }
}

#[test]
fn retired_federation_and_memory_command_groups_are_rejected() {
    let command = <Args as clap::CommandFactory>::command();
    for name in ["federate", "memory-store", "memory-stores", "memory"] {
        assert!(command.find_subcommand(name).is_none(), "unexpected {name}");
        assert!(
            Args::try_parse_from(["warp", name]).is_err(),
            "accepted retired command {name}"
        );
    }
}

#[test]
fn debug_before_subcommand_parses() {
    // Regression test: `warp --debug <subcommand>` should work.
    // Global flags like --debug must not prevent subcommand detection.
    let args = Args::try_parse_from(["warp", "--debug", "completions"]).unwrap();

    assert!(args.debug());
    assert!(matches!(args.command, Some(Command::Completions { .. })));
}

#[test]
fn harness_parse_orchestration_harness_accepts_aliases() {
    assert_eq!(
        Harness::parse_orchestration_harness("pi"),
        Some(Harness::Pi)
    );
    assert_eq!(
        Harness::parse_orchestration_harness("claude-code"),
        Some(Harness::Claude)
    );
    assert_eq!(
        Harness::parse_orchestration_harness("open_code"),
        Some(Harness::OpenCode)
    );
}

#[test]
fn harness_parse_orchestration_harness_accepts_codex() {
    assert_eq!(
        Harness::parse_orchestration_harness("codex"),
        Some(Harness::Codex)
    );
}

#[test]
#[serial_test::serial]
fn hidden_server_overrides_parse_from_env() {
    let previous_server_root = set_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, "http://localhost:8080");
    let previous_ws = set_env_var(WS_SERVER_URL_OVERRIDE_ENV, "ws://localhost:8082/graphql/v2");
    let previous_session_sharing = set_env_var(
        SESSION_SHARING_SERVER_URL_OVERRIDE_ENV,
        "ws://127.0.0.1:8081",
    );

    let args = Args::try_parse_from(["warp", "completions"]).unwrap();

    restore_env_var(SERVER_ROOT_URL_OVERRIDE_ENV, previous_server_root);
    restore_env_var(WS_SERVER_URL_OVERRIDE_ENV, previous_ws);
    restore_env_var(
        SESSION_SHARING_SERVER_URL_OVERRIDE_ENV,
        previous_session_sharing,
    );

    assert_eq!(args.server_root_url(), Some("http://localhost:8080"));
    assert_eq!(args.ws_server_url(), Some("ws://localhost:8082/graphql/v2"));
    assert_eq!(
        args.session_sharing_server_url(),
        Some("ws://127.0.0.1:8081")
    );
}

#[test]
fn builtin_sdk_commands_and_acp_bridge_are_unregistered() {
    let command = Args::clap_command();
    for name in ["agent", "environment", "environment-checkout", "mcp", "run", "model", "login", "logout", "whoami", "provider", "integration", "schedule", "secret", "harness-support", "artifact", "acp-bridge"] {
        assert!(command.find_subcommand(name).is_none(), "unexpected retired command {name}");
        assert!(Args::try_parse_from(["warp", name]).is_err(), "accepted retired command {name}");
    }
    assert!(matches!(Args::try_parse_from(["warp", "completions"]).unwrap().command(), Some(Command::Completions { .. })));
}
