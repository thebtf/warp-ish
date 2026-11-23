use std::fs;

use super::*;

fn fixture() -> tempfile::TempDir {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .find(|path| path.join(".git").is_dir())
        .unwrap();
    let scratch = root.join(".agent/tmp");
    fs::create_dir_all(&scratch).unwrap();
    tempfile::Builder::new()
        .prefix("warp-layout-import-")
        .tempdir_in(scratch)
        .unwrap()
}

fn terminal(id: u8, cwd: &str, focused: bool) -> PaneNodeSnapshot {
    PaneNodeSnapshot::Leaf(LeafSnapshot {
        is_focused: focused,
        custom_vertical_tabs_title: Some(format!("Pane {id}")),
        contents: LeafContents::Terminal(TerminalPaneSnapshot {
            uuid: vec![id; 16],
            cwd: Some(cwd.into()),
            shell_launch_data: None,
            is_active: focused,
            is_read_only: false,
        }),
    })
}

fn tab(title: &str, root: PaneNodeSnapshot, group_id: Option<TabGroupId>) -> TabSnapshot {
    TabSnapshot {
        custom_title: Some(title.into()),
        root,
        default_directory_color: None,
        selected_color: SelectedTabColor::Unset,
        left_panel: None,
        right_panel: None,
        group_id,
        pinned: true,
    }
}

fn window(
    tabs: Vec<TabSnapshot>,
    active_tab_index: usize,
    tab_groups: Vec<TabGroupSnapshot>,
) -> WindowSnapshot {
    WindowSnapshot {
        tabs,
        active_tab_index,
        bounds: Some(RectF::new(
            Vector2F::new(120., 140.),
            Vector2F::new(960., 720.),
        )),
        fullscreen_state: FullscreenState::default(),
        quake_mode: false,
        universal_search_width: None,
        voltron_width: None,
        left_panel_open: false,
        vertical_tabs_panel_open: true,
        left_panel_width: None,
        right_panel_width: None,
        tab_groups,
    }
}

fn replace_with_platform_pane(conn: &mut SqliteConnection, uuid: u8) {
    let id = schema::terminal_panes::table
        .filter(schema::terminal_panes::uuid.eq(vec![uuid; 16]))
        .select(schema::terminal_panes::id)
        .first::<i32>(conn)
        .unwrap();
    diesel::delete(schema::terminal_panes::table.find(id))
        .execute(conn)
        .unwrap();
    diesel::update(schema::pane_leaves::table.filter(schema::pane_leaves::pane_node_id.eq(id)))
        .set(schema::pane_leaves::kind.eq(model::AMBIENT_AGENT_PANE_KIND))
        .execute(conn)
        .unwrap();
}

#[test]
fn imports_ordered_local_layout_and_inert_history_without_platform_data() {
    let fixture = fixture();
    let donor = fixture.path().join("donor space#percent%.sqlite");
    let destination = fixture.path().join("destination.sqlite");
    let mut connection = setup_database(&donor).unwrap();
    let group_id = TabGroupId::new();
    let group = TabGroupSnapshot {
        id: group_id,
        name: Some("Build".into()),
        color: SelectedTabColor::Unset,
        collapsed: true,
        pinned: true,
    };
    let split = PaneNodeSnapshot::Branch(BranchSnapshot {
        direction: SplitDirection::Vertical,
        children: vec![
            (PaneFlex(1.), terminal(1, r"C:\Первый проект", false)),
            (PaneFlex(2.), terminal(2, r"C:\Retired", true)),
            (PaneFlex(3.), terminal(3, r"C:\Second Project", false)),
        ],
    });
    let donor_state = AppState {
        windows: vec![
            window(
                vec![
                    tab("Retired tab", terminal(4, r"C:\Retired Tab", true), None),
                    tab("Build tab", split, Some(group_id)),
                    tab("Debug tab", terminal(5, r"C:\Debug", true), Some(group_id)),
                ],
                1,
                vec![group],
            ),
            window(
                vec![tab("Second window", terminal(6, r"C:\Other", true), None)],
                0,
                vec![],
            ),
        ],
        active_window_index: Some(1),
        block_lists: Default::default(),
    };
    save_app_state(&mut connection, &donor_state).unwrap();
    replace_with_platform_pane(&mut connection, 2);
    replace_with_platform_pane(&mut connection, 4);
    let hostname = get_local_hostname().unwrap();
    let timestamp = chrono::NaiveDate::from_ymd_opt(2026, 11, 24)
        .unwrap()
        .and_hms_opt(14, 5, 0)
        .unwrap();
    let resume = "omp --profile fixture --resume 01234567-89ab-4cde-8fab-0123456789ab";
    let mut history_row = model::NewCommand {
        command: "Write-Output ordinary".into(),
        exit_code: Some(0),
        start_ts: Some(timestamp),
        completed_ts: Some(timestamp),
        pwd: Some(r"C:\Первый проект".into()),
        shell: Some(ShellType::PowerShell.name().into()),
        username: Some("local-user".into()),
        hostname: Some(hostname.clone()),
        session_id: Some(999),
        git_branch: Some("donor-branch".into()),
        cloud_workflow_id: None,
        workflow_command: None,
        is_agent_executed: Some(false),
    };
    let mut insert = |row: &model::NewCommand| {
        diesel::insert_into(schema::commands::table)
            .values(row)
            .execute(&mut connection)
            .unwrap();
    };
    insert(&history_row);
    history_row.command = resume.into();
    insert(&history_row);
    history_row.command = "unverified older command".into();
    history_row.is_agent_executed = None;
    insert(&history_row);
    history_row.command = "agent command".into();
    history_row.is_agent_executed = Some(true);
    insert(&history_row);
    history_row.is_agent_executed = Some(false);
    history_row.command = "cloud workflow".into();
    history_row.cloud_workflow_id = Some("cloud-id".into());
    insert(&history_row);
    history_row.cloud_workflow_id = None;
    history_row.command = "local workflow".into();
    history_row.workflow_command = Some("workflow-template".into());
    insert(&history_row);
    history_row.workflow_command = None;
    history_row.command = "remote command".into();
    history_row.hostname = Some(format!("{hostname}-remote"));
    insert(&history_row);
    // Both connections in this test share SQLite's in-process WAL index. Set an up-to-date
    // read mark before comparing the bytes, so a reader need not allocate another mark.
    assert_eq!(
        schema::commands::table
            .count()
            .get_result::<i64>(&mut connection)
            .unwrap(),
        7
    );
    let donor_before = fs::read(&donor).unwrap();
    let wal = sidecar_path(&donor, "-wal");
    let shm = sidecar_path(&donor, "-shm");
    let wal_before = fs::read(&wal).unwrap();
    let shm_before = fs::read(&shm).unwrap();
    assert!(!wal_before.is_empty());

    let summary = import_terminal_layout(&donor, &destination).unwrap();

    assert_eq!(fs::read(&donor).unwrap(), donor_before);
    assert_eq!(fs::read(&wal).unwrap(), wal_before);
    assert_eq!(fs::read(&shm).unwrap(), shm_before);
    assert_eq!(
        summary,
        WarpLayoutImportSummary {
            windows: 2,
            tab_groups: 1,
            tabs: 3,
            panes: 4,
            history_entries: 2,
            skipped_tabs: 1,
            skipped_panes: 2,
            ..WarpLayoutImportSummary::default()
        }
    );
    let mut imported = setup_database(&destination).unwrap();
    let (state, _) = read_terminal_layout(&mut imported).unwrap();
    assert_eq!(state.active_window_index, Some(1));
    let main = &state.windows[0];
    assert_eq!(main.bounds, donor_state.windows[0].bounds);
    assert_eq!(main.active_tab_index, 0);
    assert_eq!(main.tab_groups[0].name.as_deref(), Some("Build"));
    assert!(main.tab_groups[0].collapsed && main.tab_groups[0].pinned);
    assert_eq!(main.tabs[0].custom_title.as_deref(), Some("Build tab"));
    assert_eq!(main.tabs[1].custom_title.as_deref(), Some("Debug tab"));
    assert_eq!(main.tabs[0].group_id, Some(main.tab_groups[0].id));
    assert_eq!(main.tabs[1].group_id, Some(main.tab_groups[0].id));
    assert!(main.tabs[0].pinned);
    let PaneNodeSnapshot::Branch(split) = &main.tabs[0].root else {
        panic!("Expected the retained split");
    };
    assert!(matches!(split.direction, SplitDirection::Vertical));
    assert_eq!(
        split.children,
        vec![
            (PaneFlex(1.), terminal(1, r"C:\Первый проект", true)),
            (PaneFlex(3.), terminal(3, r"C:\Second Project", false)),
        ]
    );
    // The normal startup loader returns command text only to History. Restore receives
    // the layout state below, with no donor blocks or shell launch commands to replay.
    let persisted = super::super::read_sqlite_data(&mut imported).unwrap();
    assert!(persisted.app_state.unwrap().block_lists.is_empty());
    let [newest, oldest] = persisted.command_history.as_slice() else {
        panic!("Expected only the two local command history rows");
    };
    assert_eq!(newest.command, resume);
    assert_eq!(oldest.command, "Write-Output ordinary");
    assert_eq!(newest.pwd.as_deref(), Some(r"C:\Первый проект"));
    assert_eq!(newest.start_ts.as_ref().unwrap().naive_utc(), timestamp);
    assert_eq!(newest.completed_ts.as_ref().unwrap().naive_utc(), timestamp);
    assert_eq!(newest.shell_host.as_ref().unwrap().hostname, hostname);
    assert!(newest.session_id.is_none() && newest.git_branch.is_none());
    assert!(newest.workflow_command.is_none() && !newest.is_agent_executed);
}

#[test]
fn rejects_incompatible_donor_without_creating_destination() {
    let fixture = fixture();
    let donor = fixture.path().join("unrecognized.sqlite");
    let destination = fixture.path().join("destination.sqlite");
    let mut connection = setup_database(&donor).unwrap();
    connection
        .batch_execute("DELETE FROM __diesel_schema_migrations WHERE version = '20260612000000';")
        .unwrap();

    let error = import_terminal_layout(&donor, &destination).unwrap_err();

    assert!(format!("{error:#}").contains("Unsupported Warp layout schema version"));
    assert!(!destination.exists());
}

#[test]
fn repeated_import_never_overwrites_a_prior_destination() {
    let fixture = fixture();
    let donor = fixture.path().join("original.sqlite");
    let destination = fixture.path().join("staging.sqlite");
    let mut connection = setup_database(&donor).unwrap();
    save_app_state(
        &mut connection,
        &AppState {
            windows: vec![window(
                vec![tab("Terminal", terminal(1, r"C:\Project", true), None)],
                0,
                vec![],
            )],
            active_window_index: Some(0),
            block_lists: Default::default(),
        },
    )
    .unwrap();
    drop(connection);
    let wal = sidecar_path(&donor, "-wal");
    let shm = sidecar_path(&donor, "-shm");
    assert!(!wal.exists() && !shm.exists());
    let donor_before = fs::read(&donor).unwrap();
    import_terminal_layout(&donor, &destination).unwrap();
    let before = fs::read(&destination).unwrap();

    assert!(import_terminal_layout(&donor, &destination).is_err());

    assert_eq!(fs::read(&destination).unwrap(), before);
    assert_eq!(fs::read(&donor).unwrap(), donor_before);
    assert!(!wal.exists() && !shm.exists());
}

#[test]
fn import_never_uses_a_donor_executable_or_network_cwd() {
    let fixture = fixture();
    let donor = fixture.path().join("original.sqlite");
    let destination = fixture.path().join("staging.sqlite");
    let mut connection = setup_database(&donor).unwrap();
    let mut root = terminal(1, r"\\untrusted-server\share", true);
    let PaneNodeSnapshot::Leaf(LeafSnapshot {
        contents: LeafContents::Terminal(pane),
        ..
    }) = &mut root
    else {
        unreachable!();
    };
    pane.shell_launch_data = Some(ShellLaunchData::Executable {
        executable_path: PathBuf::from(r"C:\Untrusted\pwsh.exe"),
        shell_type: ShellType::PowerShell,
    });
    save_app_state(
        &mut connection,
        &AppState {
            windows: vec![window(vec![tab("Terminal", root, None)], 0, vec![])],
            active_window_index: Some(0),
            block_lists: Default::default(),
        },
    )
    .unwrap();

    let summary = import_terminal_layout(&donor, &destination).unwrap();

    assert_eq!(summary.skipped_shells, 1);
    assert_eq!(summary.skipped_cwds, 1);
    let mut conn = setup_database(&destination).unwrap();
    let (cwd, shell) = schema::terminal_panes::table
        .select((
            schema::terminal_panes::cwd,
            schema::terminal_panes::shell_launch_data,
        ))
        .first::<(Option<String>, Option<String>)>(&mut conn)
        .unwrap();
    assert_eq!((cwd, shell), (None, None));
}
