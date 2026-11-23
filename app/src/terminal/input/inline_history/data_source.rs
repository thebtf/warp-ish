//! Data source for the inline history menu, providing local shell commands.
//!
//! Ordering semantics match the legacy up-arrow history menu:
//! - Items from different sessions appear before items from the current session
//! - Within each group, items are sorted by timestamp (oldest first)
//! - Commands are deduplicated, keeping the most recent occurrence
//! - The result is that current session items appear at the bottom (closer to input)

use chrono::Local;
use ordered_float::OrderedFloat;
use warpui::{AppContext, Entity, ModelHandle, SingletonEntity};

use crate::input_suggestions::HistoryInputSuggestion;
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryFilter, QueryResult};
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::terminal::history::{History, LinkedWorkflowData};
use crate::terminal::input::inline_history::search_item::InlineHistoryItem;
use crate::terminal::input::inline_menu::{
    InlineMenuAction, InlineMenuClickBehavior, InlineMenuType,
};
use crate::terminal::model::session::active_session::ActiveSession;

#[derive(Clone, Debug)]
pub enum AcceptHistoryItem {
    Command {
        command: String,
        linked_workflow_data: Option<LinkedWorkflowData>,
    },
}

impl AcceptHistoryItem {
    pub fn buffer_replacement_text(&self) -> Option<&String> {
        match self {
            Self::Command { command, .. } => Some(command),
        }
    }
}

impl InlineMenuAction for AcceptHistoryItem {
    const MENU_TYPE: InlineMenuType = InlineMenuType::InlineHistoryMenu;
    fn click_behavior(&self) -> InlineMenuClickBehavior {
        InlineMenuClickBehavior::SelectOnClick
    }
}

/// Local shell command history, independent of agent models and view state.
pub struct InlineHistoryMenuDataSource {
    active_session: ModelHandle<ActiveSession>,
}

impl InlineHistoryMenuDataSource {
    pub fn new(active_session: ModelHandle<ActiveSession>) -> Self {
        Self { active_session }
    }
}

impl SyncDataSource for InlineHistoryMenuDataSource {
    type Action = AcceptHistoryItem;

    fn run_query(
        &self,
        query: &Query,
        app: &AppContext,
    ) -> Result<Vec<QueryResult<Self::Action>>, DataSourceRunErrorWrapper> {
        if !query.filters.is_empty() && !query.filters.contains(&QueryFilter::Commands) {
            return Ok(Vec::new());
        }
        let trimmed_query = query.text.trim();
        let session_id = self.active_session.as_ref(app).session(app).map(|s| s.id());
        let suggestions = History::handle(app)
            .as_ref(app)
            .up_arrow_command_suggestions(session_id, app);
        let mut results = Vec::new();
        for suggestion in suggestions {
            let command = suggestion.normalized_text().to_owned();
            if !trimmed_query.is_empty() && !command.starts_with(trimmed_query) {
                continue;
            }
            let HistoryInputSuggestion::Command { entry } = suggestion;
            let item = InlineHistoryItem::command(
                command,
                entry.linked_workflow_data(),
                entry.start_ts.unwrap_or_else(Local::now),
            )
            .with_prefix_match_len(trimmed_query.len())
            .with_score(OrderedFloat(results.len() as f64));
            results.push(QueryResult::from(item));
        }
        Ok(results)
    }
}

impl Entity for InlineHistoryMenuDataSource {
    type Event = ();
}

#[cfg(test)]
#[path = "data_source_tests.rs"]
mod tests;
