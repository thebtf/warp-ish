use async_channel::Sender;
use warp_terminal::shell::ShellType;
use warp_util::path::ShellFamily;

pub use warp_terminal::shell::{shell_escape_single_quotes, shell_quote_arg};

use crate::terminal::model::session::command_executor::{
    InBandCommand, InBandCommandCancelledEvent,
};

/// Formats literal environment assignments for commands launched through a shell.
/// The caller controls where the assignments are placed in the command line.
pub(super) fn serialize_environment_assignments<'a>(
    pairs: impl IntoIterator<Item = (&'a str, &'a str)>,
    shell_type: ShellType,
) -> String {
    let shell_family = ShellFamily::from(shell_type);
    pairs
        .into_iter()
        .map(|(name, value)| {
            let name = shell_family.escape(name);
            let value = match shell_family {
                ShellFamily::Posix => shell_family.escape(value),
                ShellFamily::PowerShell => shell_quote_arg(value, shell_type).into(),
            };
            match shell_type {
                ShellType::Bash | ShellType::Zsh => format!("{name}={value}"),
                ShellType::Fish => format!("set -x {name} {value};"),
                ShellType::PowerShell => format!("$env:{name} = {value};"),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

/// Set of events sent by command executors.
pub enum ExecutorCommandEvent {
    /// The command should be executed.
    ExecuteCommand {
        command: InBandCommand,
        /// A Sender that can be used to signal that the command has been cancelled.
        /// Lets us unblock the command in the executor.
        cancel_tx: Sender<InBandCommandCancelledEvent>,
    },
    /// The command identified by `id` should be cancelled.
    CancelCommand { id: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn environment_assignments_preserve_shell_syntax_and_literal_values() {
        let pairs = [("PATH", "/one two/O'Reilly/$HOME"), ("EMPTY", "")];
        let posix = r"PATH=/one\ two/O\'Reilly/\$HOME EMPTY=''";
        assert_eq!(serialize_environment_assignments(pairs, ShellType::Bash), posix);
        assert_eq!(serialize_environment_assignments(pairs, ShellType::Zsh), posix);
        assert_eq!(
            serialize_environment_assignments(pairs, ShellType::Fish),
            r"set -x PATH /one\ two/O\'Reilly/\$HOME; set -x EMPTY '';",
        );
        assert_eq!(
            serialize_environment_assignments(pairs, ShellType::PowerShell),
            "$env:PATH = '/one two/O''Reilly/$HOME'; $env:EMPTY = '';",
        );
    }
}
