use ::local_control::protocol::{
    Direction, PaneSelector, PaneTarget, TabSelector, TabTarget, TargetSelector, WindowSelector,
    WindowTarget,
};
use ::local_control::{Action, ActionKind, ControlError, ErrorCode, InstanceId, RequestEnvelope};
use serde::Deserialize;
use serde_json::{Value, json};
use warp_terminal::shell::ShellType;
use warpui::keymap::Keystroke;
use warpui::{ModelContext, ViewHandle};

use super::metadata::{self, PaneEntry, TabEntry};
use crate::local_control::LocalControlBridge;
use crate::local_control::resolver::decode_params;
use crate::terminal::TerminalView;
use crate::terminal::model::escape_sequences::KeystrokeWithDetails;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct KeyArgs {
    pane_id: String,
    key: String,
    #[serde(default)]
    modifiers: Vec<KeyModifier>,
}

#[derive(Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
enum KeyModifier {
    Ctrl,
    Alt,
    Shift,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PeerArgs {
    pane_id: String,
    program: String,
    #[serde(default)]
    args: Vec<String>,
    cwd: Option<String>,
    prompt: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SplitArgs {
    pane_id: String,
    direction: Direction,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct WindowArgs {
    window_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PaneArgs {
    pane_id: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct TabArgs {
    tab_id: String,
}

fn find_pane(
    pane_id: &str,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<PaneEntry, ControlError> {
    let tabs = tabs(ctx)?;
    let mut matching = metadata::pane_entries_for_tabs(tabs, ctx)
        .into_iter()
        .filter(|entry| entry.pane_id.to_string() == pane_id);
    let entry = matching
        .next()
        .ok_or_else(|| ControlError::new(ErrorCode::StaleTarget, "No pane has the requested ID"))?;
    if matching.next().is_some() {
        return Err(ControlError::new(
            ErrorCode::AmbiguousTarget,
            "Pane ID is ambiguous",
        ));
    }
    Ok(entry)
}

fn tabs(ctx: &mut ModelContext<LocalControlBridge>) -> Result<Vec<TabEntry>, ControlError> {
    let windows = metadata::window_entries(ctx);
    metadata::tab_entries_for_windows(windows, ActionKind::TabInspect, ctx)
}

pub(crate) fn list_layout(
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Value, ControlError> {
    let windows = metadata::window_entries(ctx);
    let window_data: Vec<_> = windows
        .iter()
        .map(|entry| json!({"id": entry.window_id.to_string()}))
        .collect();
    let tabs = metadata::tab_entries_for_windows(windows, ActionKind::TabList, ctx)?;
    let tab_data: Vec<_> = tabs
        .iter()
        .map(|entry| {
            json!({
                "id": entry.pane_group.id().to_string(), "window_id": entry.window_id.to_string()
            })
        })
        .collect();
    let pane_data: Vec<_> = metadata::pane_entries_for_tabs(tabs, ctx).into_iter().map(|entry| json!({
        "id": entry.pane_id.to_string(), "tab_id": entry.tab_id, "window_id": entry.window_id.to_string()
    })).collect();
    Ok(json!({"windows": window_data, "tabs": tab_data, "panes": pane_data}))
}

fn window_target(window_id: String) -> WindowTarget {
    WindowTarget::Id {
        id: WindowSelector(window_id),
    }
}

pub(crate) fn create_tab(
    instance_id: &Option<InstanceId>,
    params: &Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Value, ControlError> {
    let args: WindowArgs = decode_params(params)?;
    let target = TargetSelector {
        window: Some(window_target(args.window_id)),
        ..TargetSelector::default()
    };
    let mut result = super::layout::create_tab(instance_id, &json!({}), &target, ctx)?;
    let tab_id = result["tab"]["id"]
        .as_str()
        .ok_or_else(|| ControlError::new(ErrorCode::TargetStateConflict, "No tab was created"))?;
    if result["tab"]["count"] == result["tab"]["previous_count"] {
        return Err(ControlError::new(
            ErrorCode::TargetStateConflict,
            "No tab was created",
        ));
    }
    let new_tabs = tabs(ctx)?
        .into_iter()
        .filter(|tab| tab.pane_group.id().to_string() == tab_id)
        .collect();
    let pane = metadata::pane_entries_for_tabs(new_tabs, ctx)
        .into_iter()
        .next();
    if let Some(pane) = pane {
        result["pane"] = json!({"id": pane.pane_id.to_string()});
    }
    Ok(result)
}

pub(crate) fn close_tab(
    instance_id: &Option<InstanceId>,
    params: &Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Value, ControlError> {
    let args: TabArgs = decode_params(params)?;
    let entry = tabs(ctx)?
        .into_iter()
        .find(|tab| tab.pane_group.id().to_string() == args.tab_id)
        .ok_or_else(|| ControlError::new(ErrorCode::StaleTarget, "No tab has the requested ID"))?;
    let mut request = RequestEnvelope::new(Action::with_params(
        ActionKind::TabClose,
        json!({"mode": "target"}),
    )?);
    request.target.window = Some(window_target(entry.window_id.to_string()));
    request.target.tab = Some(TabTarget::Id {
        id: TabSelector(args.tab_id),
    });
    super::close::tab_close(instance_id, &request, ctx)
}

pub(crate) fn close_pane(
    params: &Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Value, ControlError> {
    let args: PaneArgs = decode_params(params)?;
    let entry = find_pane(&args.pane_id, ctx)?;
    entry.pane_group.update(ctx, |group, ctx| {
        group.close_pane_with_confirmation(entry.pane_id, ctx)
    });
    Ok(json!({"pane_id": args.pane_id, "close_requested": true}))
}

fn terminal_view(
    pane_id: &str,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<ViewHandle<TerminalView>, ControlError> {
    let entry = find_pane(pane_id, ctx)?;
    entry.pane_group.read(ctx, |group, ctx| {
        group
            .terminal_view_from_pane_id(entry.pane_id, ctx)
            .ok_or_else(|| {
                ControlError::new(
                    ErrorCode::InvalidSelector,
                    "The target pane is not a terminal",
                )
            })
    })
}

pub(crate) fn split(
    instance_id: &Option<InstanceId>,
    params: &Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Value, ControlError> {
    let args: SplitArgs = decode_params(params)?;
    let entry = find_pane(&args.pane_id, ctx)?;
    let target = TargetSelector {
        window: Some(WindowTarget::Id {
            id: WindowSelector(entry.window_id.to_string()),
        }),
        tab: Some(TabTarget::Id {
            id: TabSelector(entry.tab_id),
        }),
        pane: Some(PaneTarget::Id {
            id: PaneSelector(args.pane_id),
        }),
        ..TargetSelector::default()
    };
    let result = super::app_state::handle(
        instance_id,
        ActionKind::PaneSplit,
        &json!({"direction": args.direction}),
        &target,
        ctx,
    )?;
    if result.get("pane").is_none() {
        return Err(ControlError::new(
            ErrorCode::TargetStateConflict,
            "Split did not create a pane",
        ));
    }
    Ok(result)
}

pub(crate) fn send_key(
    params: &Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Value, ControlError> {
    let args: KeyArgs = decode_params(params)?;
    let key = match args.key.as_str() {
        "return" => "enter",
        "space" => " ",
        key => key,
    };
    let one_printable = key.chars().count() == 1 && !key.chars().any(char::is_control);
    let key = Keystroke {
        ctrl: args.modifiers.contains(&KeyModifier::Ctrl),
        alt: args.modifiers.contains(&KeyModifier::Alt),
        shift: args.modifiers.contains(&KeyModifier::Shift),
        cmd: false,
        meta: false,
        key: key.into(),
    };
    let details = KeystrokeWithDetails {
        keystroke: &key,
        key_without_modifiers: one_printable.then_some(key.key.as_str()),
        chars: (one_printable && !key.ctrl && !key.alt).then_some(key.key.as_str()),
    };
    let terminal = terminal_view(&args.pane_id, ctx)?;
    if !terminal.update(ctx, |terminal, ctx| {
        terminal.send_addressed_terminal_key(&details, ctx)
    }) {
        return Err(ControlError::new(
            ErrorCode::TargetStateConflict,
            "The target has no writable foreground program or the key cannot be encoded",
        ));
    }
    Ok(json!({ "pane_id": args.pane_id, "accepted": true }))
}

pub(crate) fn start_peer(
    params: &Value,
    ctx: &mut ModelContext<LocalControlBridge>,
) -> Result<Value, ControlError> {
    let args: PeerArgs = decode_params(params)?;
    if args.program.trim().is_empty()
        || std::iter::once(args.program.as_str())
            .chain(args.args.iter().map(String::as_str))
            .chain(args.cwd.as_deref())
            .chain(args.prompt.as_deref())
            .any(|value| value.contains('\0'))
    {
        return Err(ControlError::new(
            ErrorCode::InvalidParams,
            "Invalid program or argument",
        ));
    }
    let mut command = String::new();
    if let Some(cwd) = &args.cwd {
        command.push_str("Set-Location -ErrorAction Stop -LiteralPath ");
        command.push_str(&powershell_literal(cwd));
        command.push_str("; ");
    }
    command.push_str("& ");
    command.push_str(&powershell_literal(&args.program));
    for value in args
        .args
        .iter()
        .map(String::as_str)
        .chain(args.prompt.as_deref())
    {
        command.push(' ');
        command.push_str(&powershell_literal(value));
    }
    let terminal = terminal_view(&args.pane_id, ctx)?;
    let started = terminal.update(ctx, |terminal, ctx| {
        if terminal.active_session_shell_type(ctx) != Some(ShellType::PowerShell) {
            return false;
        }
        if !terminal.is_login_shell_bootstrapped()
            || terminal.has_active_long_running_command()
            || terminal.has_pending_command_or_awaiting_completion(ctx)
        {
            return false;
        }
        let block_env = warp_ish_mcp_transport::BLOCK_ID_ENV;
        let block = powershell_literal(&terminal.mcp_active_block_id());
        let command = format!(
            "& {{ $warpIshBlock = $env:{block_env}; try {{ $env:{block_env} = {block}; {command} }} finally {{ $env:{block_env} = $warpIshBlock }} }}"
        );
        terminal.input().update(ctx, |input, ctx| {
            input.buffer_text(ctx).is_empty() && input.try_execute_command(&command, ctx)
        })
    });
    if !started {
        return Err(ControlError::new(
            ErrorCode::TargetStateConflict,
            "Peer startup requires a ready, idle PowerShell pane with an empty prompt",
        ));
    }
    Ok(json!({ "pane_id": args.pane_id, "accepted": true }))
}

fn powershell_literal(value: &str) -> String {
    let mut literal = String::from("'");
    for ch in value.chars() {
        if matches!(ch, '\'' | '\u{2018}' | '\u{2019}' | '\u{201a}' | '\u{201b}') {
            literal.push(ch);
        }
        literal.push(ch);
    }
    literal.push('\'');
    literal
}
