use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::{self, Metadata, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, ensure};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use toml::Table;
use warp_core::ui::theme::WarpTheme;
use warpui::keymap::Keystroke;

use crate::launch_configs::launch_config::{LaunchConfig, PaneMode, PaneTemplateType};
use crate::settings::schema_generation::settings_schema_json_for_product;
use crate::tab_configs::TabConfig;
use crate::tab_configs::tab_config::TabConfigPaneType;

const MAX_FILE_SIZE: u64 = 4 * 1024 * 1024;

pub(crate) struct SettingsPaths {
    pub settings_toml: PathBuf,
    pub data_dir: PathBuf,
    pub keybindings_yaml: PathBuf,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct ImportCount {
    pub imported: usize,
    pub skipped: usize,
    // An invalid whole input file has no trustworthy entry count.
    pub unreadable_files: usize,
}

#[derive(Debug, Default, Serialize)]
pub(crate) struct SettingsImportReport {
    pub settings_keys: ImportCount,
    pub tab_configs: ImportCount,
    pub launch_configs: ImportCount,
    pub themes: ImportCount,
    pub keybindings: ImportCount,
}

pub(crate) fn project_local_files(
    donor: &SettingsPaths,
    staging: &SettingsPaths,
    staging_roots: &[&Path],
) -> Result<SettingsImportReport> {
    for root in staging_roots {
        ensure!(
            plain_dir(root)?,
            "settings import requires existing staging roots"
        );
    }
    for path in [
        &staging.settings_toml,
        &staging.data_dir,
        &staging.keybindings_yaml,
    ] {
        ensure!(
            staging_roots
                .iter()
                .any(|root| path.strip_prefix(root).is_ok_and(safe_relative)),
            "settings import path is outside staging"
        );
    }

    let mut report = SettingsImportReport::default();
    let mut selected_settings = Table::new();
    if let Some(input) = read_text(&donor.settings_toml, &mut report.settings_keys)? {
        if let Ok(table) = toml::from_str::<Table>(&input) {
            let schema = settings_schema_json_for_product(|flag| flag.is_enabled(), true)?;
            let schema: Value = serde_json::from_str(&schema)?;
            selected_settings =
                project_settings(table, &schema, &schema, "", &mut report.settings_keys);
        } else {
            report.settings_keys.unreadable_files += 1;
        }
    }
    let general = selected_settings
        .entry("general")
        .or_insert_with(|| toml::Value::Table(Table::new()));
    general
        .as_table_mut()
        .context("general settings must be a table")?
        .insert("restore_session".into(), toml::Value::Boolean(true));
    write_new(
        &staging.settings_toml,
        &toml::to_string_pretty(&selected_settings)?,
    )?;

    if let Some(input) = read_text(&donor.keybindings_yaml, &mut report.keybindings)? {
        if let Ok(bindings) = serde_yaml::from_str::<BTreeMap<String, serde_yaml::Value>>(&input) {
            let selected: BTreeMap<_, _> = bindings
                .into_iter()
                .filter(|(name, trigger)| {
                    let keep = compatible_binding(name, trigger);
                    if keep {
                        report.keybindings.imported += 1;
                    } else {
                        report.keybindings.skipped += 1;
                    }
                    keep
                })
                .collect();
            if !selected.is_empty() {
                write_new(
                    &staging.keybindings_yaml,
                    &serde_yaml::to_string(&selected)?,
                )?;
            }
        } else {
            report.keybindings.unreadable_files += 1;
        }
    }

    for (folder, extensions, project, count) in [
        (
            "tab_configs",
            &["toml"][..],
            project_tab_config as fn(&str) -> Option<String>,
            &mut report.tab_configs,
        ),
        (
            "launch_configurations",
            &["yaml", "yml"][..],
            project_launch_config,
            &mut report.launch_configs,
        ),
        (
            "themes",
            &["yaml", "yml"][..],
            project_theme,
            &mut report.themes,
        ),
    ] {
        project_folder(
            &donor.data_dir.join(folder),
            &staging.data_dir.join(folder),
            extensions,
            project,
            count,
        )?;
    }
    Ok(report)
}

fn project_settings(
    table: Table,
    node: &Value,
    root: &Value,
    prefix: &str,
    counts: &mut ImportCount,
) -> Table {
    table
        .into_iter()
        .filter_map(|(key, value)| {
            let path = if prefix.is_empty() {
                key.clone()
            } else {
                format!("{prefix}.{key}")
            };
            let spec = &node["properties"][&key];
            // Generated hierarchy tables lack the GUI annotation present on each setting.
            if !spec.is_null() && spec["x-warp-surfaces"].is_null() {
                if let toml::Value::Table(table) = value {
                    let projected = project_settings(table, spec, root, &path, counts);
                    return (!projected.is_empty()).then(|| (key, toml::Value::Table(projected)));
                }
                counts.skipped += 1;
                return None;
            }
            if spec.is_null() {
                counts.skipped += setting_leaves(&value);
                return None;
            }
            if compatible_setting(&path)
                && serde_json::to_value(&value).is_ok_and(|instance| {
                    let mut spec = spec.clone();
                    if let Some(definitions) = root.get("$defs") {
                        spec["$defs"] = definitions.clone();
                    }
                    close_schema_objects(&mut spec);
                    jsonschema::draft202012::validate(&spec, &instance).is_ok()
                })
            {
                counts.imported += 1;
                Some((key, value))
            } else {
                counts.skipped += 1;
                None
            }
        })
        .collect()
}

fn compatible_setting(path: &str) -> bool {
    // Imported templates and custom executable paths must not run on startup.
    !matches!(
        path,
        "general.default_session_mode"
            | "general.default_tab_config_path"
            | "general.login_item"
            | "general.restore_session"
            | "session.startup_shell_override"
            | "session.new_session_shell_override"
            | "session.working_directory_config"
            | "appearance.tabs.directory_tab_colors"
            | "appearance.themes.theme"
            | "appearance.themes.selected_system_themes"
            | "code.editor.use_warp_as_default_editor"
            | "code.editor.open_file_editor"
            | "code.editor.open_code_panels_file_editor"
            | "privacy.custom_secret_regex_list"
            | "global_hotkey.dedicated_window.settings"
    )
}

fn close_schema_objects(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if map.contains_key("properties") && !map.contains_key("additionalProperties") {
                map.insert("additionalProperties".into(), json!(false));
            }
            for child in map.values_mut() {
                close_schema_objects(child);
            }
        }
        Value::Array(values) => values.iter_mut().for_each(close_schema_objects),
        _ => {}
    }
}

fn setting_leaves(value: &toml::Value) -> usize {
    match value {
        toml::Value::Table(table) if !table.is_empty() => table.values().map(setting_leaves).sum(),
        _ => 1,
    }
}

fn compatible_binding(name: &str, trigger: &serde_yaml::Value) -> bool {
    // Exact ordinary bindings registered by the Windows workspace and terminal.
    const BINDINGS: &[&str] = &[
        "workspace:new_tab",
        "workspace:close_window",
        "workspace:close_active_tab",
        "workspace:close_other_tabs",
        "terminal:alternate_terminal_paste",
        "terminal:paste",
        "terminal:copy",
        "terminal:copy_commands",
        "terminal:copy_outputs",
    ];
    let Some(trigger) = trigger.as_str() else {
        return false;
    };
    BINDINGS.contains(&name)
        && (trigger == "none"
            || (!trigger.trim().is_empty()
                && trigger
                    .split_whitespace()
                    .all(|stroke| Keystroke::parse(stroke).is_ok())))
}

fn project_tab_config(input: &str) -> Option<String> {
    let config: TabConfig = toml::from_str(input).ok()?;
    let root = config.panes.first()?;
    let nodes: HashMap<_, _> = config.panes.iter().map(|node| (&node.id, node)).collect();
    if nodes.len() != config.panes.len() {
        return None;
    }
    let mut pending = vec![&root.id];
    let mut visited = HashSet::new();
    while let Some(id) = pending.pop() {
        if !visited.insert(id) {
            return None;
        }
        let node = nodes.get(id)?;
        if node
            .pane_type
            .as_ref()
            .is_some_and(|mode| *mode != TabConfigPaneType::Terminal)
            || !node.directory.as_deref().is_none_or(safe_stored_path)
            || !node.shell.as_deref().is_none_or(safe_stored_path)
        {
            return None;
        }
        match (node.split.as_ref(), node.children.as_ref()) {
            (Some(_), Some(children)) if children.len() >= 2 => pending.extend(children),
            (None, None) if node.pane_type == Some(TabConfigPaneType::Terminal) => {}
            _ => return None,
        }
    }
    if visited.len() != nodes.len() {
        return None;
    }
    toml::to_string_pretty(&config).ok()
}

fn project_launch_config(input: &str) -> Option<String> {
    let mut output = String::new();
    for document in serde_yaml::Deserializer::from_str(input) {
        let mut config = LaunchConfig::deserialize(document).ok()?;
        if config.windows.is_empty()
            || config
                .active_window_index
                .is_some_and(|i| i >= config.windows.len())
        {
            return None;
        }
        for window in &mut config.windows {
            if window.tabs.is_empty()
                || window
                    .active_tab_index
                    .is_some_and(|i| i >= window.tabs.len())
            {
                return None;
            }
            for tab in &mut window.tabs {
                if !compatible_layout(&tab.layout) {
                    return None;
                }
                // The typed serializer omits legacy tab-level commands. Keep them as
                // startup-pane commands with the same execution order on explicit launch.
                tab.layout = tab.layout_with_tab_commands();
                tab.commands.clear();
            }
        }
        let yaml = serde_yaml::to_string(&config).ok()?;
        if !yaml.starts_with("---\n") {
            output.push_str("---\n");
        }
        output.push_str(&yaml);
    }
    (!output.is_empty()).then_some(output)
}

fn compatible_layout(layout: &PaneTemplateType) -> bool {
    match layout {
        PaneTemplateType::PaneTemplate {
            cwd,
            pane_mode,
            shell,
            ..
        } => {
            *pane_mode == PaneMode::Terminal
                && cwd.to_str().is_some_and(safe_stored_path)
                && shell.as_deref().is_none_or(safe_stored_path)
        }
        PaneTemplateType::PaneBranchTemplate { panes, .. } => {
            panes.len() >= 2 && panes.iter().all(compatible_layout)
        }
    }
}

fn project_theme(input: &str) -> Option<String> {
    let theme: WarpTheme = serde_yaml::from_str(input).ok()?;
    // External image sources can refer to donor-only or network paths.
    if theme.background_image().is_some() {
        return None;
    }
    serde_yaml::to_string(&theme).ok()
}

fn safe_stored_path(path: &str) -> bool {
    if path.is_empty() {
        return true;
    }
    let path = match path.as_bytes() {
        [drive, b':', b'\\' | b'/', ..] if drive.is_ascii_alphabetic() => &path[3..],
        _ if path.starts_with(['\\', '/']) => return false,
        _ => path,
    };
    path.split(['\\', '/'])
        .all(|part| part.is_empty() || part == "." || safe_name(part))
}

fn safe_name(name: &str) -> bool {
    if name.is_empty()
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "<>:\"/\\|?*".contains(c))
    {
        return false;
    }
    let stem = name
        .split('.')
        .next()
        .unwrap_or_default()
        .trim_end_matches(' ')
        .to_ascii_uppercase();
    !matches!(
        stem.as_str(),
        "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$"
    ) && !stem
        .strip_prefix("COM")
        .or_else(|| stem.strip_prefix("LPT"))
        .is_some_and(|number| {
            matches!(
                number,
                "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"
            )
        })
}

fn safe_relative(path: &Path) -> bool {
    path.components().next().is_some()
        && path.components().all(
            |part| matches!(part, Component::Normal(name) if name.to_str().is_some_and(safe_name)),
        )
}

fn is_reparse(metadata: &Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt as _;
        const FILE_ATTRIBUTE_REPARSE_POINT: u32 = 0x0400;
        metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
    }
    #[cfg(not(windows))]
    metadata.file_type().is_symlink()
}

fn plain_dir(path: &Path) -> Result<bool> {
    ensure!(
        path.is_absolute(),
        "settings import requires absolute paths"
    );
    #[cfg(windows)]
    {
        use std::path::Prefix;

        ensure!(
            matches!(path.components().next(), Some(Component::Prefix(prefix))
                if matches!(prefix.kind(), Prefix::Disk(_) | Prefix::VerbatimDisk(_))),
            "settings import requires a local drive path"
        );
    }
    ensure!(
        !path.components().any(|part| part == Component::ParentDir),
        "settings import rejects parent-directory paths"
    );
    for ancestor in path
        .ancestors()
        .filter(|part| !part.as_os_str().is_empty())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
    {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.is_dir() && !is_reparse(&metadata) => {}
            Ok(_) => anyhow::bail!("settings import directory is not plain"),
            Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(false),
            Err(err) => return Err(err.into()),
        }
    }
    Ok(true)
}

fn read_text(path: &Path, count: &mut ImportCount) -> Result<Option<String>> {
    let parent = path
        .parent()
        .context("settings import source file has no parent")?;
    if !plain_dir(parent)? {
        return Ok(None);
    }
    let meta = match fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    if !meta.is_file() || is_reparse(&meta) || meta.len() > MAX_FILE_SIZE {
        count.unreadable_files += 1;
        return Ok(None);
    }
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt as _;

        const FILE_FLAG_OPEN_REPARSE_POINT: u32 = 0x0020_0000;
        options.custom_flags(FILE_FLAG_OPEN_REPARSE_POINT);
    }
    let file = options.open(path)?;
    let opened = file.metadata()?;
    if !opened.is_file() || is_reparse(&opened) || opened.len() > MAX_FILE_SIZE {
        count.unreadable_files += 1;
        return Ok(None);
    }
    let mut bytes = Vec::new();
    file.take(MAX_FILE_SIZE + 1).read_to_end(&mut bytes)?;
    let input = String::from_utf8(bytes)
        .ok()
        .filter(|text| text.len() as u64 <= MAX_FILE_SIZE);
    if input.is_none() {
        count.unreadable_files += 1;
    }
    Ok(input)
}

fn write_new(path: &Path, contents: &str) -> Result<()> {
    let parent = path
        .parent()
        .context("settings import destination file has no parent")?;
    if !plain_dir(parent)? {
        fs::create_dir_all(parent)?;
    }
    ensure!(
        plain_dir(parent)?,
        "settings import destination directory disappeared"
    );
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)?
        .write_all(contents.as_bytes())?;
    Ok(())
}

fn project_folder(
    source: &Path,
    destination: &Path,
    extensions: &[&str],
    project: fn(&str) -> Option<String>,
    count: &mut ImportCount,
) -> Result<()> {
    if !plain_dir(source)? {
        return Ok(());
    }
    let mut walker = walkdir::WalkDir::new(source)
        .follow_links(false)
        .min_depth(1)
        .sort_by_file_name()
        .into_iter();
    while let Some(entry) = walker.next() {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let metadata = fs::symlink_metadata(entry.path())?;
        if metadata.is_dir() && (is_reparse(&metadata) || !safe_relative(relative)) {
            walker.skip_current_dir();
        }
        if metadata.is_dir() {
            continue;
        }
        if !entry
            .path()
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| extensions.contains(&ext))
        {
            continue;
        }
        if !safe_relative(relative) {
            count.skipped += 1;
            continue;
        }
        if let Some(input) = read_text(entry.path(), count)? {
            if let Some(contents) = project(&input) {
                write_new(&destination.join(relative), &contents)?;
                count.imported += 1;
            } else {
                count.skipped += 1;
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "settings_tests.rs"]
mod tests;
