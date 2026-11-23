use std::sync::Arc;

use chrono::{Duration, Local};
use warp_core::SessionId;
use warpui::App;

use super::*;
use crate::suggestions::ignored_suggestions_model::{IgnoredSuggestionsModel, SuggestionType};
use crate::terminal::HistoryEntry;
use crate::terminal::model::session::command_executor::NoOpCommandExecutor;
use crate::terminal::model::session::{Session, SessionInfo, Sessions};
use crate::terminal::model_events::ModelEventDispatcher;

#[test]
fn empty_and_prefix_queries_return_ranked_commands_without_ai_models() {
    App::test((), |mut app| async move {
        let session_id = SessionId::from(1);
        let mut info = SessionInfo::new_for_test();
        info.session_id = session_id;
        let session = Arc::new(Session::new(
            info.clone(),
            Arc::new(NoOpCommandExecutor::default()),
        ));
        let mut other_info = info.clone();
        other_info.session_id = SessionId::from(2);
        let other_session = Arc::new(Session::new(
            other_info,
            Arc::new(NoOpCommandExecutor::default()),
        ));
        let sessions = app.add_model(|_| {
            let mut sessions = Sessions::new_for_test();
            sessions.register_session_for_test(info);
            sessions
        });
        let (_tx, rx) = async_channel::unbounded();
        let dispatcher = app.add_model(|ctx| ModelEventDispatcher::new(rx, sessions.clone(), ctx));
        dispatcher.update(&mut app, |dispatcher, _| {
            dispatcher.set_active_session_id(session_id)
        });
        let active_session = app.add_model(|ctx| ActiveSession::new(sessions, dispatcher, ctx));
        let source = app.add_model(|_| InlineHistoryMenuDataSource::new(active_session));
        let history = app.add_singleton_model(|_| History::default());
        let (tx, rx) = async_channel::bounded(1);
        app.update(|ctx| {
            ctx.subscribe_to_model(&history, move |_, event, _| {
                let crate::terminal::HistoryEvent::Initialized(id) = event;
                let _ = tx.try_send(*id);
            });
            history.update(ctx, |history, ctx| history.init_session_with(other_session, async { Vec::new() }, ctx));
        });
        assert_eq!(rx.recv().await.unwrap(), SessionId::from(2));
        let now = Local::now();
        let entry = |command: &str, seconds, id, restored| {
            HistoryEntry::command_at_time(
                command.into(),
                now + Duration::seconds(seconds),
                id,
                restored,
            )
        };
        let workflow = HistoryEntry::from(crate::terminal::history::PersistedCommand {
            id: 1,
            command: "git status".into(),
            exit_code: None,
            start_ts: Some(now + Duration::seconds(2)),
            completed_ts: None,
            pwd: None,
            shell_host: None,
            session_id: Some(session_id),
            git_branch: None,
            workflow_command: Some("git {{subcommand}}".into()),
            is_agent_executed: false,
        });
        history.update(&mut app, |history, _| {
            history.append_commands(
                SessionId::from(2),
                vec![entry("git other", 20, Some(SessionId::from(2)), false)],
            );
        });
        history.update(&mut app, |history, ctx| {
            history.init_session_with(session, async { Vec::new() }, ctx);
        });
        assert_eq!(rx.recv().await.unwrap(), session_id);
        history.update(&mut app, |history, _| {
            history.append_restored_commands(session_id, vec![entry("git restored", -10, None, true)]);
            let entries = vec![
                entry("git status", 0, Some(session_id), false),
                entry("git ignored", 1, Some(session_id), false),
                workflow,
                entry("echo local", 3, Some(session_id), false),
            ];
            for entry in entries {
                history.append_commands(session_id, vec![entry]);
            }
        });
        app.add_singleton_model(|_| {
            IgnoredSuggestionsModel::new(vec![("git ignored".into(), SuggestionType::ShellCommand)])
        });
        source.read(&app, |source, ctx| {
            // No AgentViewController is constructed: the real source owns only ActiveSession.
            let query = |text: &str, filters| {
                source
                    .run_query(
                        &Query {
                            text: text.into(),
                            filters,
                        },
                        ctx,
                    )
                    .unwrap()
                    .into_iter()
                    .map(|result| match result.accept_result() {
                        AcceptHistoryItem::Command {
                            command,
                            linked_workflow_data,
                        } => (command, linked_workflow_data),
                    })
                    .collect::<Vec<_>>()
            };
            let expected = vec![
                ("git other".into(), None),
                ("git restored".into(), None),
                (
                    "git status".into(),
                    Some(LinkedWorkflowData::Command("git {{subcommand}}".into())),
                ),
                ("echo local".into(), None),
            ];
            assert_eq!(query("", Default::default()), expected);
            assert_eq!(query(" git ", Default::default()), expected[..3]);
            assert_eq!(
                query(
                    "git",
                    std::collections::HashSet::from([QueryFilter::Commands])
                ),
                expected[..3]
            );
            assert!(query("missing", Default::default()).is_empty());
            assert!(
                query(
                    "",
                    std::collections::HashSet::from([QueryFilter::Conversations])
                )
                .is_empty()
            );
        });
    });
}
