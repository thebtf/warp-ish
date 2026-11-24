use std::collections::{HashMap, HashSet};
use std::fs::{File, OpenOptions};
use std::io::{self, ErrorKind};
use std::os::windows::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Component, Path, PathBuf, Prefix};

use anyhow::{Context, Result, ensure};
use diesel::{
    Connection, ExpressionMethods, QueryDsl, RunQueryDsl, SelectableHelper,
    connection::{DefaultLoadingMode, SimpleConnection},
    sqlite::SqliteConnection,
};
use num_traits::FromPrimitive;
use pathfinder_geometry::{rect::RectF, vector::Vector2F};
use url::Url;
use warp_terminal::shell::ShellType;
use warpui::{
    platform::FullscreenState,
    windowing::{MIN_WINDOW_HEIGHT, MIN_WINDOW_WIDTH},
};

use super::{COMMANDS_COUNT_LIMIT, model, save_app_state, schema, setup_database};
use crate::app_state::{
    AppState, BranchSnapshot, LeafContents, LeafSnapshot, PaneFlex, PaneNodeSnapshot,
    SplitDirection, TabGroupSnapshot, TabSnapshot, TerminalPaneSnapshot, WindowSnapshot,
};
use crate::persistence::block_list::{MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION, save_block};
use crate::tab::SelectedTabColor;
use crate::terminal::ShellLaunchData;
use crate::terminal::model::block::{SerializedAgentViewVisibility, SerializedBlock};
use crate::terminal::model::session::get_local_hostname;
use crate::themes::theme::AnsiColorIdentifier;
use crate::workspace::tab_group::TabGroupId;

#[derive(Debug, Default, PartialEq, Eq)]
pub(crate) struct WarpLayoutImportSummary {
    pub windows: usize,
    pub tab_groups: usize,
    pub tabs: usize,
    pub panes: usize,
    pub history_entries: usize,
    pub blocks: usize,
    pub skipped_windows: usize,
    pub skipped_tabs: usize,
    pub skipped_panes: usize,
    pub skipped_shells: usize,
    pub skipped_cwds: usize,
}

#[derive(diesel::QueryableByName)]
struct LayoutVersion {
    #[diesel(sql_type = diesel::sql_types::Bool)]
    supported: bool,
}

#[derive(diesel::Queryable, diesel::Selectable)]
#[diesel(table_name = schema::windows)]
struct LayoutWindow {
    id: i32,
    active_tab_index: i32,
    window_width: Option<f32>,
    window_height: Option<f32>,
    origin_x: Option<f32>,
    origin_y: Option<f32>,
    fullscreen_state: i32,
    vertical_tabs_panel_open: Option<bool>,
}

#[derive(diesel::Queryable, diesel::Selectable)]
#[diesel(table_name = schema::commands)]
struct HistoryCommand {
    command: String,
    exit_code: Option<i32>,
    start_ts: Option<chrono::NaiveDateTime>,
    completed_ts: Option<chrono::NaiveDateTime>,
    pwd: Option<String>,
    username: Option<String>,
}

const FILE_SHARE_READ: u32 = 0x1;
const FILE_SHARE_WRITE: u32 = 0x2;
const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x400;

fn open_regular_file(path: &Path, share_mode: u32) -> io::Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .share_mode(share_mode)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidData,
            "Expected a regular file",
        ));
    }
    Ok(file)
}

fn sidecar_path(database: &Path, suffix: &str) -> PathBuf {
    let mut name = database.as_os_str().to_os_string();
    name.push(suffix);
    name.into()
}

fn open_sidecar(database: &Path, suffix: &str, share_mode: u32) -> io::Result<Option<File>> {
    match open_regular_file(&sidecar_path(database, suffix), share_mode) {
        Ok(file) => Ok(Some(file)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Imports saved terminal layout and local terminal history into a new staging database.
pub(crate) fn import_terminal_layout(
    donor_db: &Path,
    destination_db: &Path,
) -> Result<WarpLayoutImportSummary> {
    let mut donor_url = Url::from_file_path(donor_db)
        .map_err(|_| anyhow::anyhow!("The Warp workspace database path must be absolute"))?;
    // Keep the database and both existing WAL sidecars from being replaced or deleted while
    // SQLite is reading. The VFS independently opens the WAL with CREATE even in mode=ro.
    let share_live = FILE_SHARE_READ | FILE_SHARE_WRITE;
    let main = open_regular_file(donor_db, share_live).context("Opening the Warp database")?;
    let wal = open_sidecar(donor_db, "-wal", share_live).context("Opening the Warp WAL")?;
    let shm = open_sidecar(donor_db, "-shm", share_live).context("Opening the Warp WAL index")?;
    if let (Some(_wal), Some(_shm)) = (wal, shm) {
        donor_url
            .query_pairs_mut()
            .extend_pairs([("mode", "ro"), ("readonly_shm", "1")]);
        let mut donor = SqliteConnection::establish(donor_url.as_str())
            .context("Opening the live Warp database read-only")?;
        return copy_workspace(&mut donor, destination_db);
    }
    drop(main);

    // SQLite may need to initialize WAL sidecars for a closed database. Do this only in the
    // importer-owned staging directory. Deny all writer and delete opens while copying.
    let mut main = open_regular_file(donor_db, FILE_SHARE_READ)
        .context("Close Warp before importing its workspace without both WAL sidecars")?;
    let wal = open_sidecar(donor_db, "-wal", FILE_SHARE_READ).context("Checking the Warp WAL")?;
    let journal = open_sidecar(donor_db, "-journal", FILE_SHARE_READ)
        .context("Checking the Warp rollback journal")?;
    for sidecar in [wal, journal].into_iter().flatten() {
        ensure!(
            sidecar.metadata()?.len() == 0,
            "Warp has uncheckpointed changes. Open and close Warp, then retry the import."
        );
    }
    let parent = destination_db
        .parent()
        .context("Finding the import staging directory")?;
    let snapshot = tempfile::Builder::new()
        .prefix("warp-donor-")
        .tempdir_in(parent)
        .context("Creating an isolated Warp snapshot directory")?;
    let snapshot_path = snapshot.path().join("workspace.sqlite");
    let mut snapshot_file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&snapshot_path)?;
    io::copy(&mut main, &mut snapshot_file).context("Copying the closed Warp database")?;
    drop((main, snapshot_file));
    let mut donor = SqliteConnection::establish(
        snapshot_path
            .to_str()
            .context("Reading the Warp snapshot path")?,
    )
    .context("Opening the isolated Warp snapshot")?;
    copy_workspace(&mut donor, destination_db)
}

fn copy_workspace(
    donor: &mut SqliteConnection,
    destination_db: &Path,
) -> Result<WarpLayoutImportSummary> {
    donor.batch_execute("PRAGMA query_only=ON; PRAGMA busy_timeout=1000;")?;
    donor.transaction(|donor| {
        let (state, mut summary, pane_uuids) =
            read_terminal_layout(donor).context("Reading the saved Warp terminal layout")?;
        let history = read_local_history(donor).context("Reading local Warp command history")?;

        OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(destination_db)
            .context("Creating the isolated warp-ish workspace database")?;
        let mut destination = setup_database(destination_db)?;
        destination.transaction::<_, anyhow::Error, _>(|destination| {
            save_app_state(destination, &state).context("Saving the imported terminal layout")?;
            // The normal history loader gives the newest row priority for duplicate commands.
            for command in history.iter().rev() {
                diesel::insert_into(schema::commands::table)
                    .values(command)
                    .execute(destination)
                    .context("Saving the imported command history")?;
            }
            summary.blocks = copy_local_blocks(donor, destination, pane_uuids)
                .context("Copying the saved Warp terminal blocks")?;
            Ok(())
        })?;
        summary.history_entries = history.len();
        Ok(summary)
    })
}

fn copy_local_blocks(
    donor: &mut SqliteConnection,
    destination: &mut SqliteConnection,
    pane_uuids: HashSet<Vec<u8>>,
) -> Result<usize> {
    use schema::blocks::dsl::*;

    let mut count = 0;
    for pane_uuid in pane_uuids {
        // Collect only the metadata needed to select recent blocks. A single output BLOB can
        // be large, and the cursor must be closed before fetching an individual payload.
        let mut selected = Vec::new();
        let mut seen = HashSet::new();
        let candidates = blocks
            .filter(pane_leaf_uuid.eq(&pane_uuid))
            .filter(is_local.eq(true))
            .filter(did_execute.eq(true))
            .filter(is_background.eq(false))
            .filter(completed_ts.is_not_null())
            .filter(ai_metadata.is_null())
            .order((
                diesel::dsl::sql::<diesel::sql_types::Timestamp>("COALESCE(start_ts, completed_ts)")
                    .desc(),
                id.desc(),
            ))
            .select((id, block_id, agent_view_visibility))
            .load_iter::<(Option<i32>, String, Option<String>), DefaultLoadingMode>(donor)?;
        for candidate in candidates {
            let (row_id, block_key, visibility) = candidate?;
            if let Some(row_id) = row_id
                && !block_key.is_empty()
                && !block_key.contains('\0')
                && is_terminal_block(visibility.as_deref())
                && seen.insert(block_key)
            {
                selected.push(row_id);
                if selected.len() == MAX_TERMINAL_BLOCKS_TO_PERSIST_PER_SESSION as usize {
                    break;
                }
            }
        }
        for row_id in selected.into_iter().rev() {
            let row = blocks
                .filter(id.eq(row_id))
                .select(model::Block::as_select())
                .first::<model::Block>(donor)?;
            let mut block = SerializedBlock::from(row);
            // Some older completed rows lack a start time. The restore model requires both.
            block.start_ts = block.start_ts.or(block.completed_ts);
            block.pwd = block.pwd.filter(|path| is_local_absolute(Path::new(path)));
            block.prompt_snapshot = None;
            block.agent_view_visibility = None;
            save_block(destination, pane_uuid.clone(), &block, true)?;
            count += 1;
        }
    }
    Ok(count)
}

fn is_terminal_block(visibility: Option<&str>) -> bool {
    visibility.is_none_or(|json| {
        matches!(
            serde_json::from_str::<SerializedAgentViewVisibility>(json),
            Ok(SerializedAgentViewVisibility::Terminal { .. })
        )
    })
}

fn read_local_history(conn: &mut SqliteConnection) -> Result<Vec<model::NewCommand>> {
    use schema::commands::dsl::*;

    let local_hostname = get_local_hostname()?;
    let powershell = ShellType::PowerShell.name();
    let rows = commands
        .filter(hostname.eq(&local_hostname))
        .filter(shell.eq(powershell))
        .filter(username.is_not_null())
        .filter(username.ne(""))
        .filter(cloud_workflow_id.is_null())
        .filter(workflow_command.is_null())
        .filter(is_agent_executed.eq(false))
        .order(id.desc())
        .limit(COMMANDS_COUNT_LIMIT)
        .select(HistoryCommand::as_select())
        .load::<HistoryCommand>(conn)?;

    let mut history = Vec::new();
    for row in rows {
        if row.command.trim().is_empty() || row.command.contains('\0') {
            continue;
        }
        history.push(model::NewCommand {
            command: row.command,
            exit_code: row.exit_code,
            start_ts: row.start_ts,
            completed_ts: row.completed_ts,
            pwd: row.pwd.filter(|path| is_local_absolute(Path::new(path))),
            // The history model indexes its summaries by shell, user and host.
            shell: Some(powershell.into()),
            username: row.username,
            hostname: Some(local_hostname.clone()),
            session_id: None,
            git_branch: None,
            cloud_workflow_id: None,
            workflow_command: None,
            is_agent_executed: Some(false),
        });
    }
    Ok(history)
}

fn read_terminal_layout(
    conn: &mut SqliteConnection,
) -> Result<(AppState, WarpLayoutImportSummary, HashSet<Vec<u8>>)> {
    // The grouping and pinning migrations establish the layout fields read below. Later
    // unrelated migrations and extra columns can coexist with the same topology schema.
    let version: LayoutVersion = diesel::sql_query(
        "SELECT EXISTS (SELECT 1 FROM __diesel_schema_migrations WHERE version = '20260612000000') AS supported",
    )
    .get_result(conn)
    .context("Checking the Warp layout schema version")?;
    ensure!(version.supported, "Unsupported Warp layout schema version");

    let mut summary = WarpLayoutImportSummary::default();
    let mut uuids = HashSet::new();
    let active_window = schema::app::table
        .select(schema::app::active_window_id)
        .load::<Option<i32>>(conn)?;
    let [active_window] = active_window.as_slice() else {
        anyhow::bail!("Expected one saved Warp workspace");
    };
    let windows = schema::windows::table
        .order(schema::windows::id.asc())
        .select(LayoutWindow::as_select())
        .load::<LayoutWindow>(conn)?;

    let mut state = AppState {
        windows: Vec::new(),
        active_window_index: None,
        block_lists: Default::default(),
    };
    for window in windows {
        let tabs = schema::tabs::table
            .filter(schema::tabs::window_id.eq(window.id))
            .order(schema::tabs::id.asc())
            .load::<model::Tab>(conn)?;
        let groups = schema::tab_groups::table
            .filter(schema::tab_groups::window_id.eq(window.id))
            .order(schema::tab_groups::id.asc())
            .load::<model::TabGroup>(conn)?;
        let mut imported_tabs = Vec::new();
        let mut selected_tab = None;
        let mut used_groups = HashSet::new();
        let group_ids = groups
            .iter()
            .map(|group| (group.id, TabGroupId::new()))
            .collect::<HashMap<_, _>>();
        for (index, tab) in tabs.into_iter().enumerate() {
            let nodes = schema::pane_nodes::table
                .filter(schema::pane_nodes::tab_id.eq(tab.id))
                .order(schema::pane_nodes::id.asc())
                .load::<model::PaneNode>(conn)?;
            let mut nodes_by_parent: HashMap<_, Vec<_>> = HashMap::new();
            for node in nodes {
                nodes_by_parent
                    .entry(node.parent_pane_node_id)
                    .or_default()
                    .push(node);
            }
            let mut roots = nodes_by_parent.remove(&None).unwrap_or_default();
            ensure!(
                roots.len() == 1,
                "A saved Warp tab must have exactly one root pane"
            );
            let root = read_pane(
                conn,
                roots.pop().unwrap(),
                &mut nodes_by_parent,
                &mut uuids,
                &mut summary,
                0,
            )?;
            ensure!(
                nodes_by_parent.is_empty(),
                "The saved Warp tab contains disconnected panes"
            );
            let Some(mut root) = root else {
                summary.skipped_tabs += 1;
                continue;
            };
            let mut focused = false;
            focus_pane(&mut root, true, &mut focused);
            if !focused {
                focus_pane(&mut root, false, &mut focused);
            }
            if i32::try_from(index).ok() == Some(window.active_tab_index) {
                selected_tab = Some(imported_tabs.len());
            }
            if let Some(group_id) = tab.tab_group_id {
                ensure!(
                    group_ids.contains_key(&group_id),
                    "The saved Warp tab refers to an unknown group"
                );
                used_groups.insert(group_id);
            }
            imported_tabs.push(TabSnapshot {
                custom_title: tab.custom_title,
                root,
                default_directory_color: None,
                selected_color: read_color(tab.color.as_deref()),
                left_panel: None,
                right_panel: None,
                group_id: tab.tab_group_id.map(|id| group_ids[&id]),
                pinned: tab.pinned,
            });
        }
        if imported_tabs.is_empty() {
            summary.skipped_windows += 1;
            continue;
        }
        if *active_window == Some(window.id) {
            state.active_window_index = Some(state.windows.len());
        }
        let imported_groups: Vec<_> = groups
            .into_iter()
            .filter(|group| used_groups.contains(&group.id))
            .map(|group| TabGroupSnapshot {
                id: group_ids[&group.id],
                name: group.name,
                color: read_color(group.color.as_deref()),
                collapsed: group.collapsed,
                pinned: group.pinned,
            })
            .collect();
        summary.tab_groups += imported_groups.len();
        summary.tabs += imported_tabs.len();
        state.windows.push(WindowSnapshot {
            bounds: window_bounds(&window),
            fullscreen_state: FullscreenState::from_i32(window.fullscreen_state)
                .unwrap_or_default(),
            // The Windows restore path skips quake windows. Import their terminal tabs as a regular window.
            quake_mode: false,
            active_tab_index: selected_tab.unwrap_or(0),
            tabs: imported_tabs,
            tab_groups: imported_groups,
            universal_search_width: None,
            voltron_width: None,
            left_panel_open: false,
            vertical_tabs_panel_open: window.vertical_tabs_panel_open.unwrap_or(false),
            left_panel_width: None,
            right_panel_width: None,
        });
    }
    ensure!(
        !state.windows.is_empty(),
        "Warp has no saved terminal panes to import"
    );
    state.active_window_index = Some(state.active_window_index.unwrap_or(0));
    summary.windows = state.windows.len();
    Ok((state, summary, uuids))
}

fn read_pane(
    conn: &mut SqliteConnection,
    node: model::PaneNode,
    nodes_by_parent: &mut HashMap<Option<i32>, Vec<model::PaneNode>>,
    uuids: &mut HashSet<Vec<u8>>,
    summary: &mut WarpLayoutImportSummary,
    depth: usize,
) -> Result<Option<PaneNodeSnapshot>> {
    ensure!(depth < 128, "The saved Warp pane tree is too deeply nested");
    let children = nodes_by_parent.remove(&Some(node.id)).unwrap_or_default();
    if node.is_leaf {
        ensure!(children.is_empty(), "A saved Warp pane leaf has children");
        let leaves = schema::pane_leaves::table
            .filter(schema::pane_leaves::pane_node_id.eq(node.id))
            .load::<model::PaneLeaf>(conn)?;
        let [leaf] = leaves.as_slice() else {
            anyhow::bail!("Expected exactly one record for a saved Warp pane leaf");
        };
        if leaf.kind != model::TERMINAL_PANE_KIND {
            summary.skipped_panes += 1;
            return Ok(None);
        }
        let (uuid, cwd, shell) = schema::terminal_panes::table
            .find(node.id)
            .select((
                schema::terminal_panes::uuid,
                schema::terminal_panes::cwd,
                schema::terminal_panes::shell_launch_data,
            ))
            .first::<(Vec<u8>, Option<String>, Option<String>)>(conn)?;
        ensure!(
            uuid.len() == 16 && uuids.insert(uuid.clone()),
            "Invalid or duplicated Warp terminal identity"
        );
        let local_cwd = cwd.filter(|cwd| {
            let local = is_local_absolute(Path::new(cwd));
            summary.skipped_cwds += usize::from(!local);
            local
        });
        let shell_launch_data = shell.as_deref().and_then(trusted_shell);
        summary.skipped_shells += usize::from(shell.is_some() && shell_launch_data.is_none());
        summary.panes += 1;
        return Ok(Some(PaneNodeSnapshot::Leaf(LeafSnapshot {
            is_focused: leaf.is_focused,
            custom_vertical_tabs_title: leaf.custom_vertical_tabs_title.clone(),
            contents: LeafContents::Terminal(TerminalPaneSnapshot {
                uuid,
                cwd: local_cwd,
                shell_launch_data,
                is_active: false,
                is_read_only: false,
            }),
        })));
    }
    ensure!(
        children.len() >= 2,
        "A saved Warp split must have at least two children"
    );
    let branches = schema::pane_branches::table
        .filter(schema::pane_branches::pane_node_id.eq(node.id))
        .select(schema::pane_branches::horizontal)
        .load::<bool>(conn)?;
    let [horizontal] = branches.as_slice() else {
        anyhow::bail!("Expected exactly one record for a saved Warp split");
    };
    let mut projected = Vec::new();
    for child in children {
        let flex = child.flex.unwrap_or(1.);
        ensure!(flex.is_finite() && flex > 0., "Invalid Warp split size");
        if let Some(pane) = read_pane(conn, child, nodes_by_parent, uuids, summary, depth + 1)? {
            projected.push((PaneFlex(flex), pane));
        }
    }
    Ok(match projected.len() {
        0 => None,
        1 => projected.pop().map(|(_, node)| node),
        _ => Some(PaneNodeSnapshot::Branch(BranchSnapshot {
            direction: if *horizontal {
                SplitDirection::Horizontal
            } else {
                SplitDirection::Vertical
            },
            children: projected,
        })),
    })
}

fn focus_pane(node: &mut PaneNodeSnapshot, saved_focus: bool, assigned: &mut bool) {
    match node {
        PaneNodeSnapshot::Branch(branch) => {
            for (_, child) in &mut branch.children {
                focus_pane(child, saved_focus, assigned);
            }
        }
        PaneNodeSnapshot::Leaf(leaf) => {
            leaf.is_focused = !*assigned && (!saved_focus || leaf.is_focused);
            *assigned |= leaf.is_focused;
            if let LeafContents::Terminal(pane) = &mut leaf.contents {
                pane.is_active = leaf.is_focused;
            }
        }
    }
}

fn read_color(value: Option<&str>) -> SelectedTabColor {
    value
        .and_then(|value| {
            serde_yaml::from_str(value).ok().or_else(|| {
                serde_yaml::from_str::<AnsiColorIdentifier>(value)
                    .ok()
                    .map(SelectedTabColor::Color)
            })
        })
        .unwrap_or_default()
}

fn window_bounds(window: &LayoutWindow) -> Option<RectF> {
    let (width, height, x, y) = (
        window.window_width?,
        window.window_height?,
        window.origin_x?,
        window.origin_y?,
    );
    (width >= MIN_WINDOW_WIDTH
        && height >= MIN_WINDOW_HEIGHT
        && [width, height, x, y].into_iter().all(f32::is_finite))
    .then(|| RectF::new(Vector2F::new(x, y), Vector2F::new(width, height)))
}

fn is_local_absolute(path: &Path) -> bool {
    path.is_absolute()
        && matches!(path.components().next(),
        Some(Component::Prefix(prefix)) if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_)))
}

fn trusted_shell(value: &str) -> Option<ShellLaunchData> {
    let ShellLaunchData::Executable {
        executable_path,
        shell_type: ShellType::PowerShell,
    } = serde_json::from_str::<ShellLaunchData>(value).ok()?
    else {
        return None;
    };
    // Resolve only target-owned conventional executable paths. A donor can contain custom
    // executables, WSL commands, or Docker initialization, none of which may run during import.
    let trusted_paths = [
        ("ProgramFiles", r"PowerShell\7\pwsh.exe"),
        (
            "SystemRoot",
            r"System32\WindowsPowerShell\v1.0\powershell.exe",
        ),
    ];
    let path = trusted_paths
        .into_iter()
        .filter_map(|(base, suffix)| {
            std::env::var_os(base).map(|base| PathBuf::from(base).join(suffix))
        })
        .find(|path| {
            is_local_absolute(path)
                && executable_path
                    .to_string_lossy()
                    .eq_ignore_ascii_case(&path.to_string_lossy())
                && path.is_file()
        })?;
    Some(ShellLaunchData::Executable {
        executable_path: path,
        shell_type: ShellType::PowerShell,
    })
}

#[cfg(test)]
#[path = "warp_import_tests.rs"]
mod tests;
