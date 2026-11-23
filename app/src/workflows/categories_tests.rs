use std::sync::Arc;

use warpui::{App, platform::WindowStyle};

use crate::appearance::Appearance;
use crate::user_config::WarpConfig;
use crate::workflows::WorkflowSource;
use crate::workflows::WorkflowType;
use crate::workflows::categories::{CategoriesView, WorkflowMatchType, WorkflowViewType};
use crate::workflows::workflow::Workflow;

#[test]
fn test_workflow_matches() {
    let workflow = Arc::new(WorkflowType::Local(Workflow {
        name: "g workflow_name it ".into(),
        command: "command_name git".to_string(),
        tags: vec!["foo".into(), "bar".into()],
        description: None,
        arguments: vec![],
        source_url: None,
        author: None,
        author_url: None,
        shells: vec![],
    }));

    assert_eq!(
        CategoriesView::matches_workflow(&workflow, "foo"),
        WorkflowMatchType::Tag
    );
    assert_eq!(
        CategoriesView::matches_workflow(&workflow, "bar"),
        WorkflowMatchType::Tag
    );

    // The Workflow name has higher precedence than the command.
    assert!(matches!(
        CategoriesView::matches_workflow(&workflow, "name"),
        WorkflowMatchType::Name { .. }
    ));

    // Git matches both the name and the command, but fuzzy matches command with a higher score.
    assert!(matches!(
        CategoriesView::matches_workflow(&workflow, "git"),
        WorkflowMatchType::Command { .. }
    ));

    assert!(matches!(
        CategoriesView::matches_workflow(&workflow, "command"),
        WorkflowMatchType::Command { .. }
    ));

    assert!(matches!(
        CategoriesView::matches_workflow(&workflow, "command"),
        WorkflowMatchType::Command { .. }
    ));

    assert_eq!(
        CategoriesView::matches_workflow(&workflow, "gibberish"),
        WorkflowMatchType::Unmatched
    );
}

#[test]
fn my_workflows_shows_local_commands_without_cloud_models() {
    App::test((), |mut app| async move {
        app.add_singleton_model(WarpConfig::mock);
        app.add_singleton_model(|_| Appearance::mock());

        let (_, view) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
            CategoriesView::new(
                [
                    Workflow::new("Zulu", "git status"),
                    Workflow::new("alpha", "git log"),
                ],
                [Workflow::new("App workflow", "pwd")],
                ctx,
            )
        });

        view.update(&mut app, |view, ctx| {
            view.set_focused_workflow_type(&WorkflowViewType::Local, ctx);

            let workflows = view
                .active_workflows
                .iter()
                .map(|(workflow, source)| (workflow.as_workflow().name(), *source))
                .collect::<Vec<_>>();
            assert_eq!(
                workflows,
                [("alpha", WorkflowSource::Local), ("Zulu", WorkflowSource::Local)],
            );
        });
    });
}
