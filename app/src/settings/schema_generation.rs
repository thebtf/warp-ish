use std::io::Write as _;
use std::path::Path;

use anyhow::{Context as _, Result};
use schemars::SchemaGenerator;
use serde_json::{Map, Value};
use settings::schema::SettingSchemaEntry;
use settings::{SettingSurfaces, SettingsMode};
use tempfile::NamedTempFile;

#[path = "product_policy.rs"]
pub(crate) mod product_policy;
use warp_core::channel::ChannelState;
use warp_core::features::FeatureFlag;

/// Writes the settings schema to a file or prints it to standard output.
pub fn dump_settings_schema(output_path: Option<&Path>) -> Result<()> {
    let output = settings_schema_json(|flag| flag.is_enabled())?;

    if let Some(path) = output_path {
        write_atomically(path, output.as_bytes())?;
        eprintln!("Wrote settings schema to {}", path.display());
    } else {
        println!("{output}");
    }

    Ok(())
}

fn settings_schema_json(is_flag_enabled: impl Fn(FeatureFlag) -> bool) -> Result<String> {
    settings_schema_json_for_product(is_flag_enabled, ChannelState::is_windows_warp_ish())
}

fn settings_schema_json_for_product(
    is_flag_enabled: impl Fn(FeatureFlag) -> bool,
    terminal_only: bool,
) -> Result<String> {
    let mut generator = SchemaGenerator::default();
    let mut root_properties = Map::new();
    let mut entry_count = 0;

    for entry in inventory::iter::<SettingSchemaEntry> {
        if entry.is_private {
            continue;
        }

        if terminal_only
            && (!(entry.supported_platforms_fn)().matches_current_platform()
                || !(entry.surfaces_fn)().includes(SettingsMode::Gui)
                || !product_policy::is_terminal_public_setting(entry.hierarchy, entry.storage_key))
        {
            continue;
        }

        if entry
            .feature_flag
            .is_some_and(|flag| !is_flag_enabled(flag))
        {
            continue;
        }

        // Project mixed types before invoking their original schema function, so
        // excluded agent variants do not leave reachable definitions behind.
        let mut schema_value = if terminal_only {
            terminal_setting_schema(entry, &mut generator)
        } else {
            (entry.schema_fn)(&mut generator).to_value()
        };
        schema_value
            .as_object_mut()
            .expect("setting schema should be an object")
            .insert(
                "x-warp-surfaces".to_string(),
                Value::Array(setting_surface_names(if terminal_only {
                    SettingSurfaces::GUI
                } else {
                    (entry.surfaces_fn)()
                })),
            );
        let default_json = (entry.file_default_value_fn)();

        if let Ok(mut default_value) = serde_json::from_str::<Value>(&default_json)
            && let Some(object) = schema_value.as_object_mut()
        {
            if terminal_only {
                project_terminal_default(entry, &mut default_value);
            }
            object.insert("default".to_string(), default_value);
        }

        if !entry.description.is_empty()
            && let Some(object) = schema_value.as_object_mut()
        {
            object.insert(
                "description".to_string(),
                Value::String(entry.description.to_string()),
            );
        }

        let target = if let Some(hierarchy) = entry.hierarchy {
            ensure_hierarchy(&mut root_properties, hierarchy, terminal_only)
        } else {
            &mut root_properties
        };

        target.insert(entry.storage_key.to_string(), schema_value);
        entry_count += 1;
    }

    let definitions = generator.take_definitions(true);
    let mut root = Map::new();
    root.insert(
        "$schema".to_string(),
        Value::String("https://json-schema.org/draft/2020-12/schema".to_string()),
    );
    root.insert(
        "title".to_string(),
        Value::String(if terminal_only {
            "warp-ish Settings".to_string()
        } else {
            "Warp Settings".to_string()
        }),
    );
    root.insert(
        "description".to_string(),
        Value::String(format!(
            "JSON Schema for {} settings ({} channel, {entry_count} settings)",
            if terminal_only {
                "warp-ish Windows terminal"
            } else {
                "Warp"
            },
            ChannelState::channel()
        )),
    );
    root.insert("type".to_string(), Value::String("object".to_string()));
    root.insert("properties".to_string(), Value::Object(root_properties));
    if terminal_only {
        root.insert("additionalProperties".to_string(), Value::Bool(false));
    }

    if !definitions.is_empty() {
        root.insert("$defs".to_string(), Value::Object(definitions));
    }

    let mut root_value = Value::Object(root);
    strip_numeric_metadata(&mut root_value);
    strip_empty_enum_entries(&mut root_value);

    serde_json::to_string_pretty(&root_value).context("settings schema should serialize")
}

// These are schema projections only; the original shared settings types remain
// available to upstream channels. Public file loading needs the same product policy.
#[derive(schemars::JsonSchema)]
#[allow(dead_code)]
#[schemars(rename_all = "snake_case")]
enum TerminalHeaderToolbarItem {
    TabsPanel,
    ToolsPanel,
    CodeReview,
}

#[derive(schemars::JsonSchema)]
#[allow(dead_code)]
#[schemars(rename_all = "snake_case")]
enum TerminalHeaderToolbar {
    Default,
    Custom {
        left: Vec<TerminalHeaderToolbarItem>,
        right: Vec<TerminalHeaderToolbarItem>,
    },
}

#[derive(schemars::JsonSchema, serde::Deserialize)]
#[allow(dead_code)]
#[serde(default, deny_unknown_fields)]
struct TerminalNotificationPreferences {
    // Store the file-form default (snake_case), while reusing the original mode schema.
    #[schemars(with = "crate::terminal::session_settings::NotificationsMode")]
    mode: String,
    is_long_running_enabled: bool,
    long_running_threshold: u64,
    is_password_prompt_enabled: bool,
    is_needs_attention_enabled: bool,
    play_notification_sound: bool,
}

impl Default for TerminalNotificationPreferences {
    fn default() -> Self {
        use settings_value::SettingsValue as _;

        let defaults = crate::terminal::session_settings::NotificationsSettings::default();
        Self {
            mode: defaults.mode.to_file_value().as_str().unwrap().to_owned(),
            is_long_running_enabled: defaults.is_long_running_enabled,
            long_running_threshold: defaults.long_running_threshold.as_secs(),
            is_password_prompt_enabled: defaults.is_password_prompt_enabled,
            is_needs_attention_enabled: defaults.is_needs_attention_enabled,
            play_notification_sound: defaults.play_notification_sound,
        }
    }
}

fn terminal_setting_schema(entry: &SettingSchemaEntry, generator: &mut SchemaGenerator) -> Value {
    match (entry.hierarchy, entry.storage_key) {
        (Some("general"), "default_session_mode") => {
            serde_json::json!({"type": "string", "enum": ["terminal", "tab_config"]})
        }
        (Some("notifications"), "preferences") => generator
            .subschema_for::<TerminalNotificationPreferences>()
            .to_value(),
        (Some("appearance.tabs"), "header_toolbar_chip_selection") => generator
            .subschema_for::<TerminalHeaderToolbar>()
            .to_value(),
        _ => (entry.schema_fn)(generator).to_value(),
    }
}

fn project_terminal_default(entry: &SettingSchemaEntry, value: &mut Value) {
    match (entry.hierarchy, entry.storage_key) {
        (Some("general"), "default_session_mode") => *value = Value::String("terminal".into()),
        (Some("notifications"), "preferences") => {
            if let Some(object) = value.as_object_mut() {
                object.remove("is_agent_task_completed_enabled");
            }
        }
        _ => {}
    }
}

fn setting_surface_names(surfaces: SettingSurfaces) -> Vec<Value> {
    [(SettingsMode::Gui, "gui"), (SettingsMode::Tui, "tui")]
        .into_iter()
        .filter(|(mode, _)| surfaces.includes(*mode))
        .map(|(_, name)| Value::String(name.to_owned()))
        .collect()
}

fn ensure_hierarchy<'a>(
    root_properties: &'a mut Map<String, Value>,
    hierarchy: &str,
    terminal_only: bool,
) -> &'a mut Map<String, Value> {
    let mut current = root_properties;

    for segment in hierarchy.split('.') {
        let entry = current.entry(segment.to_string()).or_insert_with(|| {
            Value::Object({
                let mut object = Map::new();
                object.insert("type".to_string(), Value::String("object".to_string()));
                object.insert("properties".to_string(), Value::Object(Map::new()));
                if terminal_only {
                    object.insert("additionalProperties".to_string(), Value::Bool(false));
                }
                object
            })
        });

        current = entry
            .as_object_mut()
            .expect("hierarchy node should be an object")
            .entry("properties")
            .or_insert_with(|| Value::Object(Map::new()))
            .as_object_mut()
            .expect("properties should be an object");
    }

    current
}

fn strip_numeric_metadata(value: &mut Value) {
    match value {
        Value::Object(map) => {
            let is_numeric = map
                .get("type")
                .and_then(Value::as_str)
                .is_some_and(|value_type| value_type == "integer" || value_type == "number");

            if is_numeric {
                map.remove("minimum");
                map.remove("maximum");
                map.remove("format");
            }

            for value in map.values_mut() {
                strip_numeric_metadata(value);
            }
        }
        Value::Array(array) => {
            for value in array {
                strip_numeric_metadata(value);
            }
        }
        _ => {}
    }
}

fn strip_empty_enum_entries(value: &mut Value) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Array(one_of)) = map.get_mut("oneOf") {
                one_of.retain(|entry| {
                    !matches!(entry, Value::Object(object)
                        if object.get("enum").is_some_and(|value| value.as_array().is_some_and(Vec::is_empty))
                    )
                });
            }

            for value in map.values_mut() {
                strip_empty_enum_entries(value);
            }
        }
        Value::Array(array) => {
            for value in array {
                strip_empty_enum_entries(value);
            }
        }
        _ => {}
    }
}

fn write_atomically(path: &Path, contents: &[u8]) -> Result<()> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create {}", parent.display()))?;

    let mut temporary_file = NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create a temporary file in {}", parent.display()))?;
    temporary_file
        .write_all(contents)
        .with_context(|| format!("failed to write temporary schema for {}", path.display()))?;
    temporary_file
        .persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("failed to persist settings schema to {}", path.display()))?;

    Ok(())
}

#[cfg(test)]
#[path = "schema_generation_tests.rs"]
mod tests;
