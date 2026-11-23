use std::path::PathBuf;
use std::sync::Arc;

use ::persistence::workspace_metadata::WorkspaceMetadata;
use chrono::{Local, Utc};
use diesel::connection::SimpleConnection;
use pathfinder_geometry::rect::RectF;
use pathfinder_geometry::vector::Vector2F;

use super::{
    AI_DOCUMENT_PANE_KIND, AI_FACT_PANE_KIND, AMBIENT_AGENT_PANE_KIND,
    EXECUTION_PROFILE_EDITOR_PANE_KIND, MCP_SERVER_PANE_KIND, TERMINAL_PANE_KIND,
    model, read_node, read_root_node, schema,
    app_database_file_path, database_file_path_for_current_scope, database_file_path_for_scope,
    decode_path, deduplicate_events, encode_path, get_all_codebase_index_metadata,
    read_sqlite_data, save_app_state, save_codebase_index_metadata, setup_database, start_writer,
};
use crate::app_state::{
    AppState, BranchSnapshot, CodePaneSnapShot, CodePaneTabSnapshot, LeafContents, LeafSnapshot,
    PaneFlex, PaneNodeSnapshot, SplitDirection,
    TabGroupSnapshot, TabSnapshot, TerminalPaneSnapshot, WindowSnapshot,
};
use crate::code::editor_management::CodeSource;
use crate::persistence::{
    BlockCompleted, ModelEvent, PersistenceScope, StartedCommandMetadata,
};
use crate::server::ids::ClientId;
use crate::tab::SelectedTabColor;
use crate::terminal::ShellLaunchData;
use crate::terminal::model::block::SerializedBlock;
use crate::terminal::model::session::SessionId;
use crate::themes::theme::AnsiColorIdentifier;
use crate::workspace::tab_group::TabGroupId;

#[test]
fn app_scope_database_path_matches_app_database_path() {
    assert_eq!(
        database_file_path_for_scope(&PersistenceScope::App),
        app_database_file_path()
    );
}

#[test]
fn database_path_for_current_scope_defaults_to_app_scope() {
    // Unit tests never call `persistence::initialize`, so the process-wide
    // scope defaults to `App` and ad-hoc read-only connections resolve to
    // the GUI database. (nextest runs each test in its own process, so no
    // other test can have set the scope.)
    assert_eq!(
        database_file_path_for_current_scope(),
        app_database_file_path()
    );
}

fn test_codebase_metadata(path: &str) -> WorkspaceMetadata {
    WorkspaceMetadata {
        path: PathBuf::from(path),
        navigated_ts: Some(Utc::now()),
        modified_ts: None,
        queried_ts: None,
    }
}

#[test]
fn sqlite_reads_terminal_layout_and_local_workspace_metadata() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![test_terminal_window_snapshot(false)],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");

    let metadata = test_codebase_metadata("/tmp/local-repo");
    save_codebase_index_metadata(&mut conn, metadata.clone())
        .expect("codebase index metadata should save");
    let terminal = read_sqlite_data(&mut conn)
        .expect("terminal data should load");
    assert_eq!(terminal.app_state.expect("terminal layout").windows.len(), 1);
    assert_eq!(terminal.local_workspace_metadata[0].path, metadata.path);
}

#[test]
fn local_terminal_restore_leaves_legacy_server_experiments_untouched() {
    use diesel::prelude::*;

    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    let database_path = fixture.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");
    let legacy_value = "OLDER_VERSION_EXPERIMENT";
    diesel::insert_into(schema::server_experiments::table)
        .values(schema::server_experiments::experiment.eq(legacy_value))
        .execute(&mut conn)
        .expect("old experiment row should be present before startup");

    read_sqlite_data(&mut conn)
        .expect("terminal snapshot should load");
    drop(conn);

    let mut conn = setup_database(&database_path).expect("database should reopen");
    let stored = schema::server_experiments::table
        .select(schema::server_experiments::experiment)
        .load::<String>(&mut conn)
        .expect("old table should still be readable");
    assert_eq!(stored, [legacy_value]);
}

#[test]
fn local_terminal_scope_restores_local_state_without_loading_platform_tables() {
    use diesel::prelude::*;
    use lsp::supported_servers::LSPServerType;

    use crate::workspace::persisted_workspace::EnablementState;
    use crate::suggestions::ignored_suggestions_model::SuggestionType;

    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    let mut conn = setup_database(&fixture.path().join("warp.sqlite"))
        .expect("database should initialize");
    let repo = fixture.path().join("проект с пробелами");
    let cwd = repo.to_string_lossy().into_owned();

    let mut window = test_terminal_window_snapshot(true);
    let PaneNodeSnapshot::Leaf(leaf) = &mut window.tabs[0].root else {
        panic!("fixture should have one terminal");
    };
    let LeafContents::Terminal(terminal) = &mut leaf.contents else {
        panic!("fixture should be a terminal");
    };
    terminal.cwd = Some(cwd.clone());
    let notebook_path = repo.join("journal.md");
    window.tabs[0].root = PaneNodeSnapshot::Branch(BranchSnapshot {
        direction: SplitDirection::Horizontal,
        children: vec![
            (PaneFlex(1.), window.tabs[0].root.clone()),
            (
                PaneFlex(1.),
                PaneNodeSnapshot::Leaf(LeafSnapshot {
                    is_focused: false,
                    custom_vertical_tabs_title: None,
                    contents: LeafContents::Notebook(
                        crate::app_state::NotebookPaneSnapshot {
                            path: Some(notebook_path),
                        },
                    ),
                }),
            ),
        ],
    });
    let original_root = window.tabs[0].root.clone();
    save_app_state(
        &mut conn,
        &AppState {
            windows: vec![window],
            active_window_index: Some(0),
            block_lists: Default::default(),
        },
    )
    .expect("layout should save");

    use schema::notebook_panes as notebooks;
    let stored_id = notebooks::table
        .select(notebooks::notebook_id)
        .first::<Option<String>>(&mut conn)
        .expect("file pane row should exist");
    assert_eq!(stored_id, None, "file panes have no cloud ID");
    // Legacy rows containing both fields must still restore their file path.
    diesel::update(notebooks::table)
        .set(notebooks::notebook_id.eq(ClientId::default().sqlite_hash()))
        .execute(&mut conn)
        .expect("legacy notebook ID should save");

    super::insert_command(
        &mut conn,
        StartedCommandMetadata {
            command: "omp --resume example-session".to_owned(),
            start_ts: Some(Local::now()),
            pwd: Some(cwd.clone()),
            shell: Some("zsh".to_owned()),
            username: None,
            hostname: None,
            session_id: Some(SessionId::from(1)),
            git_branch: None,
            workflow_command: None,
            is_agent_executed: false,
        },
    )
    .expect("command should save");

    let now = chrono::DateTime::from_timestamp(1_800_000_000, 0).unwrap();
    save_codebase_index_metadata(&mut conn, test_codebase_metadata(&cwd))
        .expect("repository should save");
    let query_only_repo = fixture.path().join("query-only");
    save_codebase_index_metadata(
        &mut conn,
        WorkspaceMetadata {
            path: query_only_repo.clone(),
            navigated_ts: None,
            modified_ts: None,
            queried_ts: Some(now),
        },
    )
    .expect("legacy query-only repository should save");
    super::upsert_workspace_language_server(
        &mut conn,
        &query_only_repo,
        LSPServerType::Pyright,
        EnablementState::Yes,
    )
    .expect("enabled LSP preference for the legacy repository should save");
    super::upsert_workspace_language_server(
        &mut conn,
        &repo,
        LSPServerType::RustAnalyzer,
        EnablementState::No,
    )
    .expect("LSP preference should save");
    super::save_project(
        &mut conn,
        model::Project {
            path: cwd.clone(),
            added_ts: now.naive_utc(),
            last_opened_ts: None,
        },
    )
    .expect("local project should save");
    super::add_ignored_suggestion(&mut conn, "ls".to_owned(), SuggestionType::ShellCommand)
        .expect("ignored shell suggestion should save");
    super::add_ignored_suggestion(&mut conn, "old query".to_owned(), SuggestionType::AIQuery)
        .expect("old ignored query should save");

    use schema::object_actions as actions;
    diesel::insert_into(actions::table)
        .values((
            actions::hashed_object_id.eq("Workflow-removed-action"),
            actions::action.eq("UNRECOGNIZED_LEGACY_ACTION"),
            actions::data.eq(Some("opaque payload")),
        ))
        .execute(&mut conn)
        .expect("old action should be preserved as a raw row");
    conn.batch_execute("ALTER TABLE object_actions RENAME TO retired_object_actions")
        .expect("retired history should be isolated within the fixture");

    conn.batch_execute("ALTER TABLE user_profiles RENAME TO retired_user_profiles")
        .expect("platform table should be isolated within the fixture");
    let restored = read_sqlite_data(&mut conn)
        .expect("terminal restore should not query retired tables");
    let app_state = restored.app_state.expect("terminal layout should load");
    assert_eq!(app_state.active_window_index, Some(0));
    assert_eq!(app_state.windows[0].tabs[0].root, original_root);
    assert!(app_state.windows[0].vertical_tabs_panel_open);
    assert_eq!(restored.command_history.len(), 1);
    assert_eq!(restored.command_history[0].command, "omp --resume example-session");
    assert_eq!(restored.command_history[0].pwd.as_deref(), Some(cwd.as_str()));
    assert_eq!(restored.local_workspace_metadata.len(), 2);
    let navigated = restored
        .local_workspace_metadata
        .iter()
        .find(|metadata| metadata.path == repo)
        .expect("locally navigated repository should restore");
    assert!(navigated.navigated_ts.is_some());
    let query_only = restored
        .local_workspace_metadata
        .iter()
        .find(|metadata| metadata.path == query_only_repo)
        .expect("query-only repository should restore for the editor");
    assert!(query_only.navigated_ts.is_none());
    assert!(query_only.modified_ts.is_none());
    assert_eq!(query_only.queried_ts, Some(now));
    assert_eq!(
        restored.workspace_language_servers[&query_only_repo][&LSPServerType::Pyright],
        EnablementState::Yes
    );
    assert_eq!(
        restored.workspace_language_servers[&repo][&LSPServerType::RustAnalyzer],
        EnablementState::No
    );
    assert_eq!(restored.projects[0].path, cwd);
    assert_eq!(
        restored.ignored_suggestions,
        vec![("ls".to_owned(), SuggestionType::ShellCommand)]
    );

    save_app_state(&mut conn, &app_state)
        .expect("terminal layout should save without the retired action table");
    conn.batch_execute("ALTER TABLE retired_object_actions RENAME TO object_actions")
        .expect("old action table should still exist");
    let old_action = actions::table
        .select((actions::action, actions::data))
        .first::<(String, Option<String>)>(&mut conn)
        .expect("local reads and writes should preserve the old action row");
    assert_eq!(
        old_action,
        ("UNRECOGNIZED_LEGACY_ACTION".to_owned(), Some("opaque payload".to_owned()))
    );
}

#[test]
fn sqlite_writer_reuses_codebase_index_metadata_events() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let conn = setup_database(&database_path).expect("database should initialize");

    let writer = start_writer(conn, database_path.clone()).expect("writer should start");
    let metadata = test_codebase_metadata("/tmp/writer-repo");
    writer
        .sender
        .send(ModelEvent::UpsertCodebaseIndexMetadata {
            index_metadata: Box::new(metadata.clone()),
        })
        .expect("upsert event should send");
    writer
        .sender
        .send(ModelEvent::Terminate)
        .expect("terminate event should send");
    writer.handle.join().expect("writer should terminate");

    let mut conn = setup_database(&database_path).expect("database should reopen");
    let restored = get_all_codebase_index_metadata(&mut conn).expect("metadata should load");
    assert_eq!(restored.len(), 1);
    assert_eq!(restored[0].path, metadata.path);

    let writer = start_writer(conn, database_path.clone()).expect("writer should restart");
    writer
        .sender
        .send(ModelEvent::DeleteCodebaseIndexMetadata {
            repo_path: metadata.path,
        })
        .expect("delete event should send");
    writer
        .sender
        .send(ModelEvent::Terminate)
        .expect("terminate event should send");
    writer.handle.join().expect("writer should terminate");

    let mut conn = setup_database(&database_path).expect("database should reopen");
    let restored = get_all_codebase_index_metadata(&mut conn).expect("metadata should load");
    assert!(restored.is_empty());
}
#[test]
fn test_deduplicate_snapshots() {
    let completed_block_1 = BlockCompleted {
        pane_id: vec![1, 2, 3],
        block: Arc::new(SerializedBlock::default()),
        is_local: true,
    };
    let completed_block_2 = BlockCompleted {
        pane_id: vec![4, 5, 6],
        block: Arc::new(SerializedBlock::default()),
        is_local: true,
    };
    let snapshot_1 = AppState {
        active_window_index: Some(1),
        block_lists: Default::default(),
        windows: Default::default(),
    };
    let snapshot_2 = AppState {
        active_window_index: Some(2),
        block_lists: Default::default(),
        windows: Default::default(),
    };
    let snapshot_3 = AppState {
        active_window_index: Some(3),
        block_lists: Default::default(),
        windows: Default::default(),
    };

    let original_events = vec![
        ModelEvent::DeleteBlocks(vec![7, 8, 9]),
        ModelEvent::Snapshot(snapshot_1.clone()),
        ModelEvent::SaveBlock(completed_block_1.clone()),
        ModelEvent::Snapshot(snapshot_2.clone()),
        ModelEvent::SaveBlock(completed_block_2.clone()),
        ModelEvent::Snapshot(snapshot_3.clone()),
        ModelEvent::DeleteBlocks(vec![10, 11, 12]),
    ];

    let filtered_events = deduplicate_events(original_events);
    assert_eq!(filtered_events.len(), 5);

    assert!(matches!(&filtered_events[0], ModelEvent::DeleteBlocks(ids) if ids == &[7, 8, 9]));
    // The first snapshot should have been filtered out.
    assert!(matches!(&filtered_events[1], &ModelEvent::SaveBlock(_)));
    // The second snapshot should have been filtered out.
    assert!(matches!(&filtered_events[2], &ModelEvent::SaveBlock(_)));
    // The third snapshot should be preserved.
    match &filtered_events[3] {
        ModelEvent::Snapshot(snapshot) => assert_eq!(snapshot, &snapshot_3),
        other => panic!("Expected ModelEvent::Snapshot, got {other:?}"),
    }
    assert!(matches!(&filtered_events[4], ModelEvent::DeleteBlocks(ids) if ids == &[10, 11, 12]));
}

#[test]
fn test_deduplicate_no_snapshots() {
    let original_events = vec![ModelEvent::SaveBlock(BlockCompleted {
        pane_id: vec![1, 2, 3],
        block: Default::default(),
        is_local: true,
    })];
    let filtered_events = deduplicate_events(original_events);
    assert_eq!(filtered_events.len(), 1);
    assert!(matches!(&filtered_events[0], &ModelEvent::SaveBlock(_)));
}

fn test_terminal_window_snapshot(vertical_tabs_panel_open: bool) -> WindowSnapshot {
    WindowSnapshot {
        tabs: vec![TabSnapshot {
            custom_title: None,
            root: PaneNodeSnapshot::Leaf(LeafSnapshot {
                is_focused: true,
                custom_vertical_tabs_title: None,
                contents: LeafContents::Terminal(TerminalPaneSnapshot {
                    uuid: vec![u8::from(vertical_tabs_panel_open) + 1],
                    cwd: Some("/tmp".to_string()),
                    shell_launch_data: Some(ShellLaunchData::Executable {
                        executable_path: PathBuf::from("/bin/zsh"),
                        shell_type: crate::terminal::shell::ShellType::Zsh,
                    }),
                    is_active: true,
                    is_read_only: false,
                }),
            }),
            default_directory_color: None,
            selected_color: SelectedTabColor::default(),
            left_panel: None,
            right_panel: None,
            group_id: None,
            pinned: false,
        }],
        active_tab_index: 0,
        bounds: None,
        fullscreen_state: Default::default(),
        quake_mode: false,
        universal_search_width: None,
        voltron_width: None,
        left_panel_open: false,
        vertical_tabs_panel_open,
        left_panel_width: None,
        right_panel_width: None,
        tab_groups: vec![],
    }
}

#[test]
fn test_sqlite_round_trips_vertical_tabs_panel_open() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![
            test_terminal_window_snapshot(false),
            test_terminal_window_snapshot(true),
        ],
        active_window_index: Some(1),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the local terminal");

    assert_eq!(restored.active_window_index, Some(1));
    assert_eq!(
        restored
            .windows
            .iter()
            .map(|window| window.vertical_tabs_panel_open)
            .collect::<Vec<_>>(),
        vec![false, true]
    );
}

#[test]
fn test_sqlite_ignores_legacy_terminal_agent_state() {
    use diesel::prelude::*;

    use crate::persistence::schema::active_mcp_servers::dsl::{active_mcp_servers, mcp_server_uuid};
    use crate::persistence::schema::terminal_panes::dsl::{
        active_conversation_id, active_profile_id, conversation_ids, input_config,
        llm_model_override, terminal_panes,
    };

    use crate::persistence::schema::windows::dsl::{
        agent_management_filters, warp_ai_width, warp_drive_index_width, windows,
    };

    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");
    let mut window = test_terminal_window_snapshot(false);
    window.universal_search_width = Some(812.);
    window.voltron_width = Some(864.);
    let app_state = AppState {
        windows: vec![window],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");
    assert_eq!(
        diesel::update(terminal_panes)
            .set((
                input_config.eq(r#"{"input_type":"AI","is_locked":true}"#),
                llm_model_override.eq("legacy-model"),
                active_profile_id.eq("legacy-profile"),
                conversation_ids.eq(r#"["legacy-conversation"]"#),
                active_conversation_id.eq("legacy-conversation"),
            ))
            .execute(&mut conn)
            .expect("legacy terminal row should update"),
        1,
    );
    let legacy_filters = r#"{"filters":{"future_filter":[true,1]}}"#;
    diesel::update(windows)
        .set((
            agent_management_filters.eq(legacy_filters),
            warp_ai_width.eq(415.),
            warp_drive_index_width.eq(315.),
        ))
        .execute(&mut conn)
        .expect("legacy window row should update");

    let legacy_mcp_uuid = "123e4567-e89b-42d3-a456-426614174000";
    diesel::insert_into(active_mcp_servers)
        .values(mcp_server_uuid.eq(legacy_mcp_uuid))
        .execute(&mut conn)
        .expect("legacy MCP row should insert");

    let restored = read_sqlite_data(&mut conn)
        .expect("legacy app state should load")
        .app_state
        .expect("app state should be present for the local terminal");
    assert_eq!(
        restored.windows[0].tabs[0].root,
        app_state.windows[0].tabs[0].root,
    );

    assert_eq!(restored.windows[0].universal_search_width, Some(812.));
    assert_eq!(restored.windows[0].voltron_width, Some(864.));
    let raw_window_fields = || windows.select((
        agent_management_filters,
        warp_ai_width,
        warp_drive_index_width,
    ));
    assert_eq!(
        raw_window_fields()
            .first::<(Option<String>, Option<f32>, Option<f32>)>(&mut conn)
            .expect("layout read should leave legacy fields untouched"),
        (Some(legacy_filters.to_owned()), Some(415.), Some(315.)),
    );

    save_app_state(&mut conn, &restored).expect("restored app state should save");
    assert_eq!(
        raw_window_fields()
            .first::<(Option<String>, Option<f32>, Option<f32>)>(&mut conn)
            .expect("replacement window should retain nullable legacy columns"),
        (None, None, None),
    );
    let resaved = read_sqlite_data(&mut conn)
        .expect("resaved app state should load")
        .app_state
        .expect("resaved local window should be present");
    assert_eq!(resaved.windows[0], restored.windows[0]);

    let panes_with_agent_state: i64 = terminal_panes
        .filter(
            llm_model_override
                .is_not_null()
                .or(active_profile_id.is_not_null())
                .or(conversation_ids.is_not_null())
                .or(active_conversation_id.is_not_null())
                .or(input_config.is_not_null()),
        )
        .count()
        .get_result(&mut conn)
        .expect("legacy columns should remain readable");
    assert_eq!(panes_with_agent_state, 0);
    let saved_legacy_mcp_uuid: String = active_mcp_servers
        .select(mcp_server_uuid)
        .first(&mut conn)
        .expect("legacy MCP row should be preserved");
    assert_eq!(saved_legacy_mcp_uuid, legacy_mcp_uuid);
}

#[test]
fn test_sqlite_preserves_last_known_terminal_cwd_by_uuid() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");
    let mut app_state = AppState {
        windows: vec![
            test_terminal_window_snapshot(false),
            test_terminal_window_snapshot(true),
        ],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    let PaneNodeSnapshot::Leaf(LeafSnapshot {
        contents: LeafContents::Terminal(pane),
        ..
    }) = &mut app_state.windows[1].tabs[0].root
    else {
        panic!("expected a terminal pane");
    };
    pane.cwd = Some(r"C:\Previous Project".to_owned());
    save_app_state(&mut conn, &app_state).expect("initial app state should save");

    app_state.windows.push(test_terminal_window_snapshot(false));
    for (window, (uuid, cwd)) in app_state
        .windows
        .iter_mut()
        .zip([(2, None), (1, Some(r"C:\New Project")), (3, None)])
    {
        let PaneNodeSnapshot::Leaf(LeafSnapshot {
            contents: LeafContents::Terminal(pane),
            ..
        }) = &mut window.tabs[0].root
        else {
            panic!("expected a terminal pane");
        };
        pane.uuid = vec![uuid];
        pane.cwd = cwd.map(str::to_owned);
    }
    save_app_state(&mut conn, &app_state).expect("updated app state should save");

    let restored = read_sqlite_data(&mut conn)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the local terminal");
    assert_eq!(restored.windows.len(), 3);
    for (window, (uuid, cwd)) in restored
        .windows
        .iter()
        .zip([
            (2, Some(r"C:\Previous Project")),
            (1, Some(r"C:\New Project")),
            (3, None),
        ])
    {
        let PaneNodeSnapshot::Leaf(LeafSnapshot {
            contents: LeafContents::Terminal(pane),
            ..
        }) = &window.tabs[0].root
        else {
            panic!("expected a terminal pane");
        };
        assert_eq!(pane.uuid, [uuid]);
        assert_eq!(pane.cwd.as_deref(), cwd);
    }
}

#[test]
fn sqlite_reads_legacy_window_without_restoring_team_affinity() {
    use diesel::prelude::*;
    use schema::windows::dsl as saved_windows;

    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    let mut conn = setup_database(&fixture.path().join("warp.sqlite"))
        .expect("database should initialize");
    let window = test_terminal_window_snapshot(false);
    let retained_tab = window.tabs[0].root.clone();
    save_app_state(
        &mut conn,
        &AppState {
            windows: vec![window],
            active_window_index: Some(0),
            block_lists: Default::default(),
        },
    )
    .expect("terminal layout should save");

    let new_row_team: Option<String> = saved_windows::windows
        .select(saved_windows::team_uid)
        .first(&mut conn)
        .expect("new window row should remain accessible");
    assert_eq!(new_row_team, None);

    let old_team = "opaque-legacy-team-id";
    diesel::update(saved_windows::windows)
        .set(saved_windows::team_uid.eq(Some(old_team)))
        .execute(&mut conn)
        .expect("legacy window team should be seeded");

    let restored = read_sqlite_data(&mut conn)
        .expect("terminal data should load")
        .app_state
        .expect("terminal layout should load");
    assert_eq!(restored.windows[0].tabs[0].root, retained_tab);
    let stored: Option<String> = saved_windows::windows
        .select(saved_windows::team_uid)
        .first(&mut conn)
        .expect("window row should remain accessible");
    assert_eq!(stored.as_deref(), Some(old_team));
}

#[test]
fn test_sqlite_round_trips_custom_vertical_tabs_title() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            tabs: vec![TabSnapshot {
                custom_title: None,
                root: PaneNodeSnapshot::Leaf(LeafSnapshot {
                    is_focused: true,
                    custom_vertical_tabs_title: Some("Production API".to_string()),
                    contents: LeafContents::Terminal(TerminalPaneSnapshot {
                        uuid: vec![42],
                        cwd: Some("/tmp".to_string()),
                        shell_launch_data: Some(ShellLaunchData::Executable {
                            executable_path: PathBuf::from("/bin/zsh"),
                            shell_type: crate::terminal::shell::ShellType::Zsh,
                        }),
                        is_active: true,
                        is_read_only: false,
                    }),
                }),
                default_directory_color: None,
                selected_color: SelectedTabColor::default(),
                left_panel: None,
                right_panel: None,
                group_id: None,
                pinned: false,
            }],
            active_tab_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
            tab_groups: vec![],
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the local terminal");

    let PaneNodeSnapshot::Leaf(LeafSnapshot {
        custom_vertical_tabs_title,
        ..
    }) = &restored.windows[0].tabs[0].root
    else {
        panic!("Expected terminal pane leaf");
    };
    assert_eq!(
        custom_vertical_tabs_title.as_deref(),
        Some("Production API")
    );
}

#[test]
fn test_sqlite_round_trips_code_pane_with_multiple_tabs() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            tabs: vec![TabSnapshot {
                custom_title: None,
                root: PaneNodeSnapshot::Leaf(LeafSnapshot {
                    is_focused: true,
                    custom_vertical_tabs_title: None,
                    contents: LeafContents::Code(CodePaneSnapShot::Local {
                        tabs: vec![
                            CodePaneTabSnapshot {
                                path: Some(PathBuf::from("/tmp/main.rs")),
                            },
                            CodePaneTabSnapshot {
                                path: Some(PathBuf::from("/tmp/lib.rs")),
                            },
                            CodePaneTabSnapshot { path: None },
                        ],
                        active_tab_index: 1,
                        source: Some(CodeSource::FileTree {
                            location: crate::code::buffer_location::LocalOrRemotePath::Local(
                                PathBuf::from("/tmp/main.rs"),
                            ),
                        }),
                    }),
                }),
                default_directory_color: None,
                selected_color: SelectedTabColor::default(),
                left_panel: None,
                right_panel: None,
                group_id: None,
                pinned: false,
            }],
            active_tab_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
            tab_groups: vec![],
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the local terminal");

    assert_eq!(restored.windows.len(), 1);
    let restored_tab = &restored.windows[0].tabs[0];
    let PaneNodeSnapshot::Leaf(LeafSnapshot {
        contents:
            LeafContents::Code(CodePaneSnapShot::Local {
                tabs,
                active_tab_index,
                source,
            }),
        ..
    }) = &restored_tab.root
    else {
        panic!("Expected code pane leaf");
    };

    assert_eq!(tabs.len(), 3);
    assert_eq!(*active_tab_index, 1);
    assert_eq!(tabs[0].path, Some(PathBuf::from("/tmp/main.rs")));
    assert_eq!(tabs[1].path, Some(PathBuf::from("/tmp/lib.rs")));
    assert_eq!(tabs[2].path, None);
    assert!(matches!(source, Some(CodeSource::FileTree { .. })));
}

/// Verifies that a tab group and its membership round-trip through save/restore.
#[test]
fn test_sqlite_round_trips_tab_groups() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let group_id = TabGroupId::new();
    let tab_in_group = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: true,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![1],
                cwd: Some("/tmp/grouped".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: true,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: Some(group_id),
        pinned: false,
    };
    let tab_outside_group = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: false,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![2],
                cwd: Some("/tmp/ungrouped".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: false,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: None,
        pinned: false,
    };

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            tabs: vec![tab_in_group, tab_outside_group],
            active_tab_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
            tab_groups: vec![TabGroupSnapshot {
                id: group_id,
                name: Some("Backend".to_string()),
                color: SelectedTabColor::Color(AnsiColorIdentifier::Blue),
                collapsed: true,
                pinned: false,
            }],
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the local terminal");

    assert_eq!(restored.windows.len(), 1);
    let restored_window = &restored.windows[0];
    assert_eq!(restored_window.tab_groups.len(), 1);
    let restored_group = &restored_window.tab_groups[0];
    assert_eq!(restored_group.name.as_deref(), Some("Backend"));
    assert_eq!(
        restored_group.color,
        SelectedTabColor::Color(AnsiColorIdentifier::Blue)
    );
    assert!(restored_group.collapsed);

    // The in-memory `TabGroupId` is minted fresh on restore, so we check that
    // the grouped tab points at the restored group, and the ungrouped tab
    // remains ungrouped.
    assert_eq!(restored_window.tabs.len(), 2);
    assert_eq!(restored_window.tabs[0].group_id, Some(restored_group.id));
    assert_eq!(restored_window.tabs[1].group_id, None);
}

/// Verifies that the `pinned` flag on tabs and tab groups round-trips through
/// save/restore so the user's pinned layout survives an app restart.
#[test]
fn test_sqlite_round_trips_pinned_state() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let pinned_group_id = TabGroupId::new();
    let unpinned_group_id = TabGroupId::new();

    let pinned_tab = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: true,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![10],
                cwd: Some("/tmp/pinned".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: true,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: None,
        pinned: true,
    };
    let unpinned_tab = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: false,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![11],
                cwd: Some("/tmp/unpinned".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: false,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: Some(unpinned_group_id),
        pinned: false,
    };
    let tab_in_pinned_group = TabSnapshot {
        custom_title: None,
        root: PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: false,
            custom_vertical_tabs_title: None,
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid: vec![12],
                cwd: Some("/tmp/pinned-group".to_string()),
                shell_launch_data: Some(ShellLaunchData::Executable {
                    executable_path: PathBuf::from("/bin/zsh"),
                    shell_type: crate::terminal::shell::ShellType::Zsh,
                }),
                is_active: false,
                is_read_only: false,
            }),
        }),
        default_directory_color: None,
        selected_color: SelectedTabColor::default(),
        left_panel: None,
        right_panel: None,
        group_id: Some(pinned_group_id),
        pinned: false,
    };

    let app_state = AppState {
        windows: vec![WindowSnapshot {
            tabs: vec![pinned_tab, tab_in_pinned_group, unpinned_tab],
            active_tab_index: 0,
            bounds: None,
            fullscreen_state: Default::default(),
            quake_mode: false,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: false,
            left_panel_width: None,
            right_panel_width: None,
            tab_groups: vec![
                TabGroupSnapshot {
                    id: pinned_group_id,
                    name: Some("Pinned".to_string()),
                    color: SelectedTabColor::default(),
                    collapsed: false,
                    pinned: true,
                },
                TabGroupSnapshot {
                    id: unpinned_group_id,
                    name: Some("Loose".to_string()),
                    color: SelectedTabColor::default(),
                    collapsed: false,
                    pinned: false,
                },
            ],
        }],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    let restored = read_sqlite_data(&mut conn)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the local terminal");

    assert_eq!(restored.windows.len(), 1);
    let restored_window = &restored.windows[0];

    // Tabs come back in insertion order; pinned flag should match what we saved.
    assert_eq!(restored_window.tabs.len(), 3);
    assert!(restored_window.tabs[0].pinned);
    assert!(!restored_window.tabs[1].pinned);
    assert!(!restored_window.tabs[2].pinned);

    // Both groups round-trip with their pinned state preserved. Group ids are
    // minted fresh on restore, so we look them up by name.
    assert_eq!(restored_window.tab_groups.len(), 2);
    let restored_pinned_group = restored_window
        .tab_groups
        .iter()
        .find(|group| group.name.as_deref() == Some("Pinned"))
        .expect("pinned group should restore");
    let restored_loose_group = restored_window
        .tab_groups
        .iter()
        .find(|group| group.name.as_deref() == Some("Loose"))
        .expect("unpinned group should restore");
    assert!(restored_pinned_group.pinned);
    assert!(!restored_loose_group.pinned);
}

fn assert_encode_then_decode_preserves_original_path(original_path: PathBuf) {
    let bytes = encode_path(original_path.clone());
    let decoded_path = decode_path(bytes);
    assert_eq!(original_path, decoded_path);
}

/// Test that a local path can be encoded and decoded. We use this when persisting a local
/// file path for notebooks in sqlite. We need this test because Windows `OsString`s are
/// often arbitrary sequences of 16-bit values, unlike Unix which uses sequences of 8-bit
/// values (bytes). Since `diesel::sql_types::Binary` deals with sequences of bytes (`u8`)
/// we need to perform special casting on `OsString`s on Windows.
#[test]
fn test_path_encode_decode() {
    // Empty path
    assert_encode_then_decode_preserves_original_path(PathBuf::new());

    // Windows-style paths
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"C:\windows\system32.dll"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("c:temp"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\emoji\🙈.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\ñoñàscii\temp.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\hindi\हिन्दी"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from(r"\temp\cjk\狗没有耐心"));

    // Unix-style paths
    assert_encode_then_decode_preserves_original_path(PathBuf::from(
        "/home/persistence/example.sql",
    ));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("./database/log.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/emoji/🙈.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/ñoñàscii/temp.txt"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/hindi/हिन्दी"));
    assert_encode_then_decode_preserves_original_path(PathBuf::from("/temp/cjk/狗没有耐心"));
}

#[test]
fn legacy_cloud_object_rows_remain_opaque_to_session_persistence() {
    use diesel::prelude::*;

    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    let mut conn = setup_database(&fixture.path().join("warp.sqlite"))
        .expect("database should initialize");
    conn.batch_execute(
        "INSERT INTO generic_string_objects(id, data) VALUES (42, 'not valid JSON');
         INSERT INTO object_metadata(
             id, is_pending, object_type, shareable_object_id, retry_count, is_welcome_object
         ) VALUES (10, 0, 'GENERIC_STRING_OBJECT_JSON_UNKNOWN_LEGACY', 42, 0, 0);
         INSERT INTO object_permissions(
             id, object_metadata_id, subject_type, subject_uid, object_guests
         ) VALUES (43, 10, 'TEAM', 'legacy-team', X'010203');",
    )
    .expect("legacy payloads should insert as raw SQLite rows");
    conn.batch_execute(
        "ALTER TABLE generic_string_objects RENAME TO retired_generic_string_objects;
         ALTER TABLE object_metadata RENAME TO retired_object_metadata;
         ALTER TABLE object_permissions RENAME TO retired_object_permissions;
         ALTER TABLE cloud_objects_refreshes RENAME TO retired_cloud_objects_refreshes;",
    )
    .expect("unavailable retired tables should be isolated within the fixture");

    let snapshot = AppState {
        windows: vec![test_terminal_window_snapshot(true)],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &snapshot)
        .expect("shell layout should save without the retired tables");
    let restored = read_sqlite_data(&mut conn)
        .expect("terminal restoration should ignore the retired tables");
    let layout = restored.app_state.expect("shell layout should restore");
    assert_eq!(layout.active_window_index, Some(0));
    assert_eq!(layout.windows[0].tabs[0].root, snapshot.windows[0].tabs[0].root);

    conn.batch_execute(
        "ALTER TABLE retired_object_metadata RENAME TO object_metadata;
         ALTER TABLE retired_object_permissions RENAME TO object_permissions;
         ALTER TABLE retired_generic_string_objects RENAME TO generic_string_objects;",
    )
    .expect("retired payload tables should still exist");
    use schema::{generic_string_objects as strings, object_metadata as metadata, object_permissions as permissions};
    let raw_type: String = metadata::table.find(10).select(metadata::object_type)
        .first(&mut conn).expect("unknown object type should survive");
    let raw_data: String = strings::table.find(42).select(strings::data)
        .first(&mut conn).expect("opaque object payload should survive");
    let raw_guests: Option<Vec<u8>> = permissions::table.find(43).select(permissions::object_guests)
        .first(&mut conn).expect("opaque permission payload should survive");
    assert_eq!(raw_type, "GENERIC_STRING_OBJECT_JSON_UNKNOWN_LEGACY");
    assert_eq!(raw_data, "not valid JSON");
    assert_eq!(raw_guests, Some(vec![1, 2, 3]));
}

// Regression: GH#10083. The macOS green-tile button could leave a 1px-wide
// window bound in `AppContext::window_bounds`, which previously round-tripped
// through SQLite and restored as an unusable 1px sliver. Bounds below the
// platform minimum window size must be dropped on save.
#[test]
fn test_sqlite_drops_too_small_bounds_on_save() {
    use diesel::prelude::*;

    use crate::persistence::schema::windows;

    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    let mut snapshot = test_terminal_window_snapshot(false);
    snapshot.bounds = Some(RectF::new(
        Vector2F::new(0.0, -1410.0),
        Vector2F::new(1.0, 1410.0),
    ));

    let app_state = AppState {
        windows: vec![snapshot],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };

    save_app_state(&mut conn, &app_state).expect("app state should save");

    // Query the row directly so the assertion isolates the save guard and is
    // not masked by the read-side guard in `read_sqlite_data`.
    let row: (Option<f32>, Option<f32>, Option<f32>, Option<f32>) = windows::dsl::windows
        .select((
            windows::columns::window_width,
            windows::columns::window_height,
            windows::columns::origin_x,
            windows::columns::origin_y,
        ))
        .first(&mut conn)
        .expect("a windows row should have been inserted");

    assert_eq!(
        row,
        (None, None, None, None),
        "save-path guard must persist NULL bound columns for sub-minimum geometry"
    );
}

// Regression: GH#10083. Users whose warp.sqlite already contains a 1px row
// (because they hit the bug on an earlier build) must still recover to default
// geometry on next launch rather than restoring the sliver.
#[test]
fn test_sqlite_drops_too_small_bounds_on_read() {
    let tempdir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = tempdir.path().join("warp.sqlite");
    let mut conn = setup_database(&database_path).expect("database should initialize");

    // Save with no bounds so a row exists, then corrupt it directly to bypass
    // the save-path guard and simulate a pre-existing bad row.
    let app_state = AppState {
        windows: vec![test_terminal_window_snapshot(false)],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &app_state).expect("app state should save");

    conn.batch_execute(
        "UPDATE windows \
         SET window_width = 1.0, window_height = 1410.0, \
             origin_x = 0.0, origin_y = -1410.0",
    )
    .expect("corrupting update should succeed");

    let restored = read_sqlite_data(&mut conn)
        .expect("app state should load")
        .app_state
        .expect("app state should be present for the local terminal");

    assert_eq!(restored.windows.len(), 1);
    assert!(
        restored.windows[0].bounds.is_none(),
        "tiny persisted bounds must be discarded on read so users recover from a corrupt DB"
    );
}

#[test]
fn terminal_snapshot_keeps_retired_account_cache_rows_opaque() {
    use diesel::prelude::*;

    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    let mut conn = setup_database(&fixture.path().join("warp.sqlite"))
        .expect("database should initialize");
    conn.batch_execute(
        "INSERT INTO teams(id, name, server_uid, billing_metadata_json) VALUES
            (1, 'Legacy team', 'legacy-team', 'opaque billing value');
         INSERT INTO team_members(team_id, user_uid, email, role, is_disabled) VALUES
            (1, 'legacy-member', 'member@example.test', 'unknown legacy role', 1);
         INSERT INTO workspaces(name, server_uid, is_selected) VALUES
            ('Legacy workspace', 'legacy-workspace', 1);
         INSERT INTO user_profiles(firebase_uid, photo_url, email) VALUES
            ('legacy-profile', 'opaque photo value', 'profile@example.test');
         INSERT INTO current_user_information(email) VALUES ('account@example.test');",
    )
    .expect("legacy fixtures should be inserted directly");

    let state = AppState {
        windows: vec![test_terminal_window_snapshot(false)],
        active_window_index: Some(0),
        block_lists: Default::default(),
    };
    save_app_state(&mut conn, &state).expect("terminal layout should save");
    let restored = read_sqlite_data(&mut conn)
        .expect("terminal state should load")
        .app_state
        .expect("terminal layout should load");
    save_app_state(&mut conn, &restored).expect("restored terminal layout should save");

    let billing: Option<String> = schema::teams::table
        .select(schema::teams::billing_metadata_json)
        .first(&mut conn)
        .expect("old billing value should remain");
    assert_eq!(billing.as_deref(), Some("opaque billing value"));
    let member: (String, bool) = schema::team_members::table
        .select((schema::team_members::role, schema::team_members::is_disabled))
        .first(&mut conn)
        .expect("old team member should remain");
    assert_eq!(member, ("unknown legacy role".to_owned(), true));
    let workspace: (String, bool) = schema::workspaces::table
        .select((schema::workspaces::server_uid, schema::workspaces::is_selected))
        .first(&mut conn)
        .expect("old workspace should remain");
    assert_eq!(workspace, ("legacy-workspace".to_owned(), true));
    let profile: (String, String) = schema::user_profiles::table
        .select((schema::user_profiles::firebase_uid, schema::user_profiles::photo_url))
        .first(&mut conn)
        .expect("old profile should remain");
    assert_eq!(profile, ("legacy-profile".to_owned(), "opaque photo value".to_owned()));
    let account: String = schema::current_user_information::table
        .select(schema::current_user_information::email)
        .first(&mut conn)
        .expect("old account row should remain");
    assert_eq!(account, "account@example.test");
}

#[test]
fn retired_pane_rows_preserve_terminal_siblings_and_do_not_rewrite_orphans() {
    use super::{ENV_VAR_COLLECTION_PANE_KIND, NOTEBOOK_PANE_KIND, WORKFLOW_PANE_KIND};
    use diesel::prelude::*;

    for (kind, with_payload) in [
        (AI_DOCUMENT_PANE_KIND, false),
        (AI_FACT_PANE_KIND, false),
        (EXECUTION_PROFILE_EDITOR_PANE_KIND, false),
        (AMBIENT_AGENT_PANE_KIND, false),
        (MCP_SERVER_PANE_KIND, false),
        (WORKFLOW_PANE_KIND, false),
        (ENV_VAR_COLLECTION_PANE_KIND, false),
        (WORKFLOW_PANE_KIND, true),
        (ENV_VAR_COLLECTION_PANE_KIND, true),
        (NOTEBOOK_PANE_KIND, false),
        (NOTEBOOK_PANE_KIND, true),
    ] {
        let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .unwrap()
            .join(".agent/tmp");
        std::fs::create_dir_all(&scratch).unwrap();
        let fixture = tempfile::Builder::new()
            .prefix("retired-pane-")
            .tempdir_in(scratch)
            .unwrap();
        let mut conn = setup_database(&fixture.path().join("warp.sqlite")).unwrap();
        let mut window = test_terminal_window_snapshot(false);
        let left = window.tabs[0].root.clone();
        let mut right = left.clone();
        let PaneNodeSnapshot::Leaf(leaf) = &mut right else {
            unreachable!()
        };
        leaf.is_focused = false;
        leaf.custom_vertical_tabs_title = Some("right terminal".into());
        let LeafContents::Terminal(terminal) = &mut leaf.contents else {
            unreachable!()
        };
        terminal.uuid = vec![2];
        terminal.is_active = false;
        terminal.cwd = Some("retained-right-directory".into());
        let mut obsolete = left.clone();
        let PaneNodeSnapshot::Leaf(leaf) = &mut obsolete else {
            unreachable!()
        };
        let LeafContents::Terminal(terminal) = &mut leaf.contents else {
            unreachable!()
        };
        leaf.is_focused = false;
        terminal.uuid = vec![9];
        terminal.is_active = false;
        window.tabs[0].root = PaneNodeSnapshot::Branch(BranchSnapshot {
            direction: SplitDirection::Horizontal,
            children: vec![
                (PaneFlex(2.), left.clone()),
                (
                    PaneFlex(3.),
                    PaneNodeSnapshot::Branch(BranchSnapshot {
                        direction: SplitDirection::Vertical,
                        children: vec![(PaneFlex(1.), obsolete)],
                    }),
                ),
                (PaneFlex(5.), right.clone()),
            ],
        });
        save_app_state(
            &mut conn,
            &AppState {
                windows: vec![window],
                active_window_index: Some(0),
                block_lists: Default::default(),
            },
        )
        .unwrap();
        let retired_id: i32 = schema::terminal_panes::table
            .filter(schema::terminal_panes::uuid.eq(vec![9u8]))
            .select(schema::terminal_panes::id)
            .first(&mut conn)
            .unwrap();
        diesel::delete(
            schema::terminal_panes::table.filter(schema::terminal_panes::id.eq(retired_id)),
        )
        .execute(&mut conn)
        .unwrap();
        diesel::update(
            schema::pane_leaves::table.filter(schema::pane_leaves::pane_node_id.eq(retired_id)),
        )
        .set(schema::pane_leaves::kind.eq(kind))
        .execute(&mut conn)
        .unwrap();
        if with_payload {
            let legacy_id = ClientId::default().sqlite_hash();
            match kind {
                WORKFLOW_PANE_KIND => {
                    use schema::workflow_panes as panes;
                    diesel::insert_into(panes::table)
                        .values((panes::id.eq(retired_id), panes::workflow_id.eq(&legacy_id)))
                        .execute(&mut conn)
                        .unwrap();
                }
                ENV_VAR_COLLECTION_PANE_KIND => {
                    use schema::env_var_collection_panes as panes;
                    diesel::insert_into(panes::table)
                        .values((
                            panes::id.eq(retired_id),
                            panes::env_var_collection_id.eq(&legacy_id),
                        ))
                        .execute(&mut conn)
                        .unwrap();
                }
                NOTEBOOK_PANE_KIND => {
                    use schema::notebook_panes as panes;
                    diesel::insert_into(panes::table)
                        .values((panes::id.eq(retired_id), panes::notebook_id.eq(&legacy_id)))
                        .execute(&mut conn)
                        .unwrap();
                }
                _ => unreachable!(),
            }
        }
        let restored = read_sqlite_data(&mut conn)
            .unwrap()
            .app_state
            .unwrap();
        assert_eq!(restored.windows.len(), 1);
        assert_eq!(restored.windows[0].tabs.len(), 1);
        assert_eq!(
            restored.windows[0].tabs[0].root,
            PaneNodeSnapshot::Branch(BranchSnapshot {
                direction: SplitDirection::Horizontal,
                children: vec![(PaneFlex(2.), left.clone()), (PaneFlex(5.), right)],
            }),
            "retired kind {kind}"
        );
        save_app_state(&mut conn, &restored).unwrap();
        let reread = read_sqlite_data(&mut conn)
            .unwrap()
            .app_state
            .unwrap();
        assert_eq!(reread, restored, "saved state after pruning {kind}");
        assert_eq!(
            schema::pane_nodes::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            3
        );
        assert_eq!(
            schema::pane_leaves::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            2
        );
        assert_eq!(
            schema::terminal_panes::table
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            2
        );
        assert_eq!(
            schema::pane_leaves::table
                .filter(schema::pane_leaves::kind.eq(kind))
                .count()
                .get_result::<i64>(&mut conn)
                .unwrap(),
            0
        );
        let root_id: i32 = schema::pane_nodes::table
            .filter(schema::pane_nodes::parent_pane_node_id.is_null())
            .select(schema::pane_nodes::id)
            .first(&mut conn)
            .unwrap();
        let right_id: i32 = schema::terminal_panes::table
            .filter(schema::terminal_panes::uuid.eq(vec![2u8]))
            .select(schema::terminal_panes::id)
            .first(&mut conn)
            .unwrap();
        diesel::delete(
            schema::terminal_panes::table.filter(schema::terminal_panes::id.eq(right_id)),
        )
        .execute(&mut conn)
        .unwrap();
        diesel::update(
            schema::pane_leaves::table.filter(schema::pane_leaves::pane_node_id.eq(right_id)),
        )
        .set(schema::pane_leaves::kind.eq(kind))
        .execute(&mut conn)
        .unwrap();
        let tab_id_val: i32 = schema::pane_nodes::table
            .find(root_id)
            .select(schema::pane_nodes::tab_id)
            .first(&mut conn)
            .unwrap();
        assert_eq!(
            read_root_node(&mut conn, tab_id_val).unwrap(),
            Some(left.clone())
        );
        let left_id: i32 = schema::terminal_panes::table
            .filter(schema::terminal_panes::uuid.eq(vec![1u8]))
            .select(schema::terminal_panes::id)
            .first(&mut conn)
            .unwrap();
        diesel::delete(
            schema::terminal_panes::table.filter(schema::terminal_panes::id.eq(left_id)),
        )
        .execute(&mut conn)
        .unwrap();
        diesel::update(
            schema::pane_leaves::table.filter(schema::pane_leaves::pane_node_id.eq(left_id)),
        )
        .set(schema::pane_leaves::kind.eq(kind))
        .execute(&mut conn)
        .unwrap();
        let tab_id_val: i32 = schema::pane_nodes::table
            .find(root_id)
            .select(schema::pane_nodes::tab_id)
            .first(&mut conn)
            .unwrap();
        assert!(read_root_node(&mut conn, tab_id_val).unwrap().is_none());
    }
}

#[test]
fn retired_tabs_remap_selection_and_unknown_or_malformed_retained_leaves_still_fail() {
    use diesel::prelude::*;
    let scratch = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .join(".agent/tmp");
    std::fs::create_dir_all(&scratch).unwrap();
    let fixture = tempfile::Builder::new()
        .prefix("retired-tab-")
        .tempdir_in(scratch)
        .unwrap();
    let mut conn = setup_database(&fixture.path().join("warp.sqlite")).unwrap();
    let mut window = test_terminal_window_snapshot(false);
    let mut selected = window.tabs[0].clone();
    let PaneNodeSnapshot::Leaf(leaf) = &mut selected.root else {
        unreachable!()
    };
    let LeafContents::Terminal(terminal) = &mut leaf.contents else {
        unreachable!()
    };
    terminal.uuid = vec![2];
    selected.custom_title = Some("selected surviving tab".into());
    window.tabs.push(selected.clone());
    window.active_tab_index = 1;
    save_app_state(
        &mut conn,
        &AppState {
            windows: vec![window],
            active_window_index: Some(0),
            block_lists: Default::default(),
        },
    )
    .unwrap();
    let first_id: i32 = schema::pane_nodes::table
        .order(schema::pane_nodes::id.asc())
        .select(schema::pane_nodes::id)
        .first(&mut conn)
        .unwrap();
    diesel::delete(schema::terminal_panes::table.filter(schema::terminal_panes::id.eq(first_id)))
        .execute(&mut conn)
        .unwrap();
    diesel::update(
        schema::pane_leaves::table.filter(schema::pane_leaves::pane_node_id.eq(first_id)),
    )
    .set(schema::pane_leaves::kind.eq(EXECUTION_PROFILE_EDITOR_PANE_KIND))
    .execute(&mut conn)
    .unwrap();
    let restored = read_sqlite_data(&mut conn)
        .unwrap()
        .app_state
        .unwrap();
    assert_eq!(restored.windows[0].tabs, vec![selected]);
    assert_eq!(restored.windows[0].active_tab_index, 0);
    let read = |conn: &mut SqliteConnection| {
        let node: model::PaneNode = schema::pane_nodes::table
            .find(first_id)
            .first(conn)
            .unwrap();
        read_node(conn, node)
    };
    assert!(read(&mut conn).unwrap().is_none());
    diesel::update(
        schema::pane_leaves::table.filter(schema::pane_leaves::pane_node_id.eq(first_id)),
    )
    .set(schema::pane_leaves::kind.eq("unknown_future_kind"))
    .execute(&mut conn)
    .unwrap();
    assert!(read(&mut conn).is_err());
    diesel::update(
        schema::pane_leaves::table.filter(schema::pane_leaves::pane_node_id.eq(first_id)),
    )
    .set(schema::pane_leaves::kind.eq(TERMINAL_PANE_KIND))
    .execute(&mut conn)
    .unwrap();
    assert!(read(&mut conn).is_err());
}
