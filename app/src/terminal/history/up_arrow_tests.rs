use std::sync::Arc;

use chrono::{Duration, Local};
use settings::Setting;
use warp_core::SessionId;

use crate::input_suggestions::HistoryInputSuggestion;
use crate::suggestions::ignored_suggestions_model::{IgnoredSuggestionsModel, SuggestionType};
use crate::terminal::model::session::command_executor::NoOpCommandExecutor;
use crate::terminal::model::session::{Session, SessionInfo};
use crate::terminal::session_settings::SessionSettings;
use crate::terminal::{History, HistoryEntry, LinkedWorkflowData};
use crate::test_util::settings::initialize_settings_for_tests;

use warpui::{App, AppContext, SingletonEntity};
fn command_entry(
    session_id: SessionId,
    command: &str,
    age: i64,
    is_agent_executed: bool,
    workflow_command: Option<&str>,
) -> HistoryEntry {
    HistoryEntry {
        session_id: Some(session_id),
        command: command.to_owned(),
        pwd: None,
        start_ts: Some(Local::now() + Duration::milliseconds(age)),
        completed_ts: None,
        exit_code: None,
        git_head: None,
        shell_host: None,
        workflow_command: workflow_command.map(str::to_owned),
        is_for_restored_block: false,
        is_agent_executed,
    }
}

fn command_history(
    session_id: SessionId,
    app: &AppContext,
) -> Vec<(String, Option<LinkedWorkflowData>)> {
    History::handle(app)
        .as_ref(app)
        .up_arrow_command_suggestions(Some(session_id), app)
        .into_iter()
        .map(|suggestion| {
            let text = suggestion.normalized_text().to_owned();
            let HistoryInputSuggestion::Command { entry } = suggestion;
            (text, entry.linked_workflow_data())
        })
        .collect()
}

async fn add_command_history(app: &mut App, session_id: SessionId, entries: Vec<HistoryEntry>) {
    let mut session_info = SessionInfo::new_for_test();
    session_info.session_id = session_id;
    let history = app.add_singleton_model(|_| History::default());
    let (initialized_tx, initialized_rx) = async_channel::unbounded();
    app.update(|ctx| {
        ctx.subscribe_to_model(&history, move |_, event, _| match event {
            crate::terminal::HistoryEvent::Initialized(id) => {
                let _ = initialized_tx.try_send(*id);
            }
        });
    });

    // Prior sessions must contribute history before this session's start index.
    let mut session_ids = Vec::new();
    for entry in &entries {
        if !entry.is_for_restored_block
            && let Some(id) = entry.session_id
            && id != session_id
            && !session_ids.contains(&id)
        {
            session_ids.push(id);
        }
    }
    session_ids.push(session_id);
    for id in session_ids {
        let mut info = session_info.clone();
        info.session_id = id;
        let session = Arc::new(Session::new(
            info,
            Arc::new(NoOpCommandExecutor::default()),
        ));
        history.update(app, |history, ctx| {
            history.init_session_with(session, async { Vec::new() }, ctx);
        });
        assert_eq!(initialized_rx.recv().await.unwrap(), id);
        history.update(app, |history, _| {
            if id == session_id {
                history.append_restored_commands(
                    id,
                    entries.iter().filter(|entry| entry.is_for_restored_block).cloned().collect(),
                );
            }
            for entry in entries.iter().filter(|entry| !entry.is_for_restored_block && entry.session_id == Some(id)) {
                history.append_commands(id, vec![entry.clone()]);
            }
        });
    }
}
#[test]
fn command_history_without_settings_preserves_order_dedup_filters_and_workflows() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(1);
        let mut restored = command_entry(SessionId::from(2), "restored", -100, false, None);
        restored.is_for_restored_block = true;
        add_command_history(
            &mut app,
            session_id,
            vec![
                command_entry(session_id, "same", -5, false, None),
                command_entry(SessionId::from(2), "other session", 10, false, None),
                restored,
                command_entry(session_id, "ignored", -4, false, None),
                command_entry(session_id, "   ", -3, false, None),
                command_entry(session_id, " same ", -2, false, Some("same {{argument}}")),
                command_entry(session_id, "agent shell", -1, true, None),
            ],
        )
        .await;
        app.add_singleton_model(|_| {
            IgnoredSuggestionsModel::new(vec![("ignored".to_owned(), SuggestionType::ShellCommand)])
        });
        app.read(|ctx| {
            assert!(!ctx.has_singleton_model::<SessionSettings>());
            assert_eq!(
                command_history(session_id, ctx),
                vec![
                    ("other session".into(), None),
                    ("restored".into(), None),
                    (
                        "same".into(),
                        Some(LinkedWorkflowData::Command("same {{argument}}".into()))
                    ),
                    ("agent shell".into(), None),
                ]
            );
        });
    });
}

#[test]
fn command_history_respects_agent_command_setting() {
    App::test((), |mut app| async move {
        initialize_settings_for_tests(&mut app);
        let session_id = SessionId::from(1);
        add_command_history(
            &mut app,
            session_id,
            vec![
                command_entry(session_id, "user command", 0, false, None),
                command_entry(session_id, "agent command", 1, true, None),
            ],
        )
        .await;

        app.read(|ctx| {
            assert_eq!(
                command_history(session_id, ctx)
                    .into_iter()
                    .map(|(text, _)| text)
                    .collect::<Vec<_>>(),
                vec!["user command"]
            );
        });

        SessionSettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .include_agent_commands_in_history
                .set_value(true, ctx)
                .unwrap();
        });
        app.read(|ctx| {
            assert_eq!(
                command_history(session_id, ctx)
                    .into_iter()
                    .map(|(text, _)| text)
                    .collect::<Vec<_>>(),
                vec!["user command", "agent command"]
            );
        });
    });
}
