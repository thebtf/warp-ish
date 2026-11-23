//! Command line for the Windows local terminal, independent of the Oz parser.
use std::io;

use clap::{CommandFactory, FromArgMatches, Parser, Subcommand};
use clap_complete::aot::{Shell, generate};

use crate::{AppArgs, ParentOpts};

#[derive(Debug, Parser)]
#[command(name = "warp-ish", about = "Local terminal for Windows")]
#[command(subcommand_precedence_over_arg = true)]
pub struct TerminalArgs {
    #[command(subcommand)]
    pub command: Option<TerminalCommand>,
    #[command(flatten)]
    pub app: AppArgs,
}

#[derive(Debug, Subcommand)]
pub enum TerminalCommand {
    /// Generate completions for this local terminal command line.
    Completions {
        #[arg(value_enum)]
        shell: Option<Shell>,
    },
    /// Print local debugging information and exit.
    #[command(long_flag = "dump-debug-info")]
    DumpDebugInfo,
    /// Print the local settings schema, or write it to a file.
    DumpSettingsSchema {
        output_path: Option<std::path::PathBuf>,
    },
    /// Run the terminal's isolated file-search worker.
    #[command(hide = true)]
    RipgrepSearch {
        #[command(flatten)]
        parent: ParentOpts,
        #[arg(long = "ignore-case")]
        ignore_case: bool,
        #[arg(long = "multiline")]
        multiline: bool,
        pattern: String,
        paths: Vec<std::path::PathBuf>,
    },
    /// Control a running local terminal instance.
    #[command(long_flag = "warpctrl")]
    Warpctrl(crate::local_control::ControlArgs),
}

impl TerminalArgs {
    pub fn clap_command() -> clap::Command {
        <Self as CommandFactory>::command().version(crate::version_string())
    }

    pub fn from_env() -> Self {
        let matches = Self::clap_command()
            .try_get_matches()
            .unwrap_or_else(|err| err.exit());
        Self::from_arg_matches(&matches).unwrap_or_else(|err| err.exit())
    }
}

pub fn generate_completions_to_stdout(shell: Option<Shell>) -> anyhow::Result<()> {
    let shell = shell.or_else(Shell::from_env).ok_or_else(|| {
        anyhow::anyhow!(
            "Could not determine shell from environment. Please provide a shell argument."
        )
    })?;
    let mut command = TerminalArgs::clap_command();
    let binary_name = crate::binary_name().unwrap_or_else(|| "warp-ish".to_owned());
    generate(shell, &mut command, binary_name, &mut io::stdout());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_parser_is_consistent_and_has_no_platform_commands() {
        TerminalArgs::clap_command().debug_assert();
        let mut command = TerminalArgs::clap_command();
        let help = command.render_long_help().to_string();
        assert!(help.contains("Local terminal for Windows"));
        for name in [
            "completions",
            "dump-debug-info",
            "dump-settings-schema",
            "ripgrep-search",
            "warpctrl",
        ] {
            assert!(command.find_subcommand(name).is_some(), "missing {name}");
        }
        for name in [
            "agent",
            "environment",
            "environment-checkout",
            "mcp",
            "run",
            "model",
            "login",
            "logout",
            "whoami",
            "provider",
            "integration",
            "schedule",
            "secret",
            "federate",
            "memory-store",
            "memory-stores",
            "memory",
            "harness-support",
            "artifact",
            "api-key",
            "runner",
            "acp-bridge",
            "remote-server-daemon",
            "remote-server-proxy",
            "print-telemetry-events",
        ] {
            assert!(command.find_subcommand(name).is_none(), "unexpected {name}");
            assert!(
                TerminalArgs::try_parse_from(["warp-ish", name]).is_err(),
                "accepted {name}"
            );
        }
        for flag in [
            "--api-key",
            "--server-root-url",
            "--ws-server-url",
            "--session-sharing-server-url",
            "--print-telemetry-events",
        ] {
            assert!(
                TerminalArgs::try_parse_from(["warp-ish", flag, "unused"]).is_err(),
                "accepted {flag}"
            );
        }
    }

    #[test]
    fn local_parser_keeps_os_arguments_and_worker_wire_format() {
        let args = TerminalArgs::try_parse_from([
            "warp-ish",
            "--finish-update",
            "warp-ish://open?path=C:/work",
        ])
        .unwrap();
        assert!(args.app.finish_update);
        assert_eq!(args.app.urls.len(), 1);
        assert!(args.command.is_none());
        let args = TerminalArgs::try_parse_from([
            "warp-ish",
            "ripgrep-search",
            "--parent-pid",
            "1234",
            "--ignore-case",
            "--multiline",
            "needle",
            "C:/work",
        ])
        .unwrap();
        let Some(TerminalCommand::RipgrepSearch {
            parent,
            ignore_case,
            multiline,
            pattern,
            paths,
        }) = args.command
        else {
            panic!("expected search worker")
        };
        assert_eq!(parent.pid, Some(1234));
        assert!(ignore_case && multiline);
        assert_eq!(pattern, "needle");
        assert_eq!(paths, [std::path::PathBuf::from("C:/work")]);
        assert!(matches!(
            TerminalArgs::try_parse_from(["warp-ish", "--dump-debug-info"])
                .unwrap()
                .command,
            Some(TerminalCommand::DumpDebugInfo)
        ));
        assert!(matches!(
            TerminalArgs::try_parse_from(["warp-ish", "--warpctrl", "instance", "list"])
                .unwrap()
                .command,
            Some(TerminalCommand::Warpctrl(_))
        ));
        assert!(matches!(
            TerminalArgs::try_parse_from([
                "warp-ish",
                "dump-settings-schema",
                "settings-schema.json"
            ])
            .unwrap()
            .command,
            Some(TerminalCommand::DumpSettingsSchema {
                output_path: Some(_)
            })
        ));
    }

    #[cfg(enable_crash_recovery)]
    #[test]
    fn local_parser_keeps_windows_recovery_parent_handle() {
        let args = TerminalArgs::try_parse_from([
            "warp-ish",
            "--crash-recovery-mechanism",
            "disable-opengl",
            "--parent-handle",
            "42",
        ])
        .unwrap();
        assert!(matches!(
            args.app.crash_recovery_mechanism,
            Some(crate::RecoveryMechanism::DisableOpenGL)
        ));
        assert!(args.app.parent.handle.is_some());
    }

    #[test]
    fn completions_describe_only_the_local_parser() {
        let mut output = Vec::new();
        generate(
            Shell::PowerShell,
            &mut TerminalArgs::clap_command(),
            "warp-ish",
            &mut output,
        );
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("dump-settings-schema"));
        assert!(text.contains("warpctrl"));
        for forbidden in [
            "run-cloud",
            "environment-checkout",
            "acp-bridge",
            "--api-key",
            "print-telemetry-events",
        ] {
            assert!(!text.contains(forbidden), "completion leaked {forbidden}");
        }
    }
}
