pub mod new_session_shell;
pub mod startup_shell;
pub mod working_directory_config;

use settings::Setting;
use warpui::{AppContext, SingletonEntity};

use instant::Duration;
use lazy_static::lazy_static;
pub use new_session_shell::*;
use serde::{Deserialize, Serialize};
pub use startup_shell::*;
use warp_core::settings::macros::define_settings_group;
use warp_core::settings::{RespectUserSyncSetting, SupportedPlatforms, SyncToCloud};
pub use working_directory_config::*;

use crate::context_chips::prompt::PromptSelection;

lazy_static! {
    pub static ref DEFAULT_THRESHOLD_FOR_LONG_RUNNING_NOTIFICATION: Duration =
        Duration::from_secs(30);
}

#[derive(
    Copy,
    Clone,
    Debug,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Whether the user has enabled or disabled notifications.",
    rename_all = "snake_case"
)]
pub enum NotificationsMode {
    // User has not been shown notifications banner before or has seen it before but decided not to dismiss it.
    #[schemars(description = "Notifications have not been configured yet.")]
    Unset,

    // User has asked not to be shown notifications banner again.
    #[schemars(description = "The notifications banner has been dismissed.")]
    Dismissed,

    // User has enabled system notifications and wants to receive notifications.
    #[schemars(description = "Notifications are enabled.")]
    Enabled,

    // User had previously enabled notifications, but has now disabled them.
    #[schemars(description = "Notifications are disabled.")]
    Disabled,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, settings_value::SettingsValue)]
/**
 * Added [serde(default)] to ensure that new notification settings are backwards compatible with old clients.
 * Otherwise, clients will fail to deserialize existing settings after updating.
 *
 * @see https://github.com/warpdotdev/warp-internal/pull/14596/files#diff-90221c7ecae01c6faf8f170158dea3e49d34d40225a306da42ccc76489d1f84cR43-R44
 *
 * Alternative considered: Using Option<bool> fields would have required more
 * complex defaulting logic to set the default value to true.
 *
 */
#[serde(default)]
#[derive(schemars::JsonSchema)]
#[schemars(description = "Notification preferences for terminal events.")]
pub struct NotificationsSettings {
    #[schemars(
        description = "Whether notifications are enabled, disabled, or not yet configured."
    )]
    pub mode: NotificationsMode,

    #[schemars(description = "Whether to notify when a long-running command completes.")]
    pub is_long_running_enabled: bool,
    #[schemars(
        with = "u64",
        description = "Threshold in seconds for long-running command notifications."
    )]
    pub long_running_threshold: Duration,

    /// Legacy. To be combined with `is_needs_attention_enabled` when desktop notifs are unflagged.
    #[schemars(description = "Whether to notify when a password prompt is detected.")]
    pub is_password_prompt_enabled: bool,

    #[schemars(description = "Whether to notify when an agent task completes.")]
    pub is_agent_task_completed_enabled: bool,
    #[schemars(description = "Whether to notify when a session needs attention.")]
    pub is_needs_attention_enabled: bool,

    #[schemars(description = "Whether to play a sound with notifications.")]
    pub play_notification_sound: bool,
}

impl Default for NotificationsSettings {
    fn default() -> Self {
        Self {
            mode: NotificationsMode::Unset,
            is_long_running_enabled: true,
            long_running_threshold: *DEFAULT_THRESHOLD_FOR_LONG_RUNNING_NOTIFICATION,
            is_password_prompt_enabled: true,
            is_agent_task_completed_enabled: true,
            is_needs_attention_enabled: true,
            play_notification_sound: true,
        }
    }
}

#[derive(
    Copy,
    Clone,
    Debug,
    Default,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
pub enum GithubPrPromptChipDefaultValidation {
    #[default]
    Unvalidated,
    Validated,
    Suppressed,
}

impl GithubPrPromptChipDefaultValidation {
    pub fn is_suppressed(self) -> bool {
        matches!(self, Self::Suppressed)
    }
}

/// The default mode for new terminal sessions.
#[derive(
    Default,
    Debug,
    serde::Serialize,
    serde::Deserialize,
    PartialEq,
    Copy,
    Clone,
    schemars::JsonSchema,
    settings_value::SettingsValue,
)]
#[schemars(
    description = "Default mode for new sessions.",
    rename_all = "snake_case"
)]
pub enum DefaultSessionMode {
    /// New sessions start in the terminal mode (default).
    #[default]
    Terminal,
    /// Legacy value for the removed built-in agent view.
    Agent,

    /// New sessions open a user-defined tab config.
    /// The specific config is identified by the companion `default_tab_config_path` setting.
    TabConfig,
    /// New sessions open in a local Docker sandbox.
    /// Requires the `LocalDockerSandbox` feature flag; falls back to `Terminal` when disabled.
    DockerSandbox,
}

settings::macros::implement_setting_for_enum!(
    DefaultSessionMode,
    SessionSettings,
    SupportedPlatforms::ALL,
    SyncToCloud::Never,
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "general.default_session_mode",
    description: "The default mode for new terminal sessions.",
);

impl DefaultSessionMode {
    /// Display name for the settings dropdown.
    pub fn display_name(&self) -> &'static str {
        match self {
            DefaultSessionMode::Terminal => "Terminal",
            DefaultSessionMode::Agent => "Agent",
            DefaultSessionMode::TabConfig => "Tab Config",
            DefaultSessionMode::DockerSandbox => "Local Docker Sandbox",
        }
    }
}

define_settings_group!(SessionSettings, settings: [
    // Use `default_session_mode()` to resolve legacy and unsupported modes.
    default_session_mode_internal: DefaultSessionMode,

    // The file path of the tab config used when default_session_mode_internal is TabConfig.
    // Only read when mode is TabConfig; ignored for all other modes.
    // Machine-local (tab config paths vary per machine), so never synced to cloud.
    default_tab_config_path: DefaultTabConfigPath {
        type: String,
        default: String::new(),
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.default_tab_config_path",
    }

    include_agent_commands_in_history: IncludeAgentCommandsInHistory {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.input.include_agent_commands_in_history",
        description: "Whether agent-executed commands are included in command history.",
    }
    working_directory_config: WorkingDirectoryConfig,
    startup_shell_override: StartupShellOverride {
        type: StartupShell,
        default: StartupShell::default(),
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "session.startup_shell_override",
        description: "The shell to use when Warp starts up.",
    },
    new_session_shell_override: NewSessionShellOverride {
        type: Option<NewSessionShell>,
        default: None,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "session.new_session_shell_override",
        description: "The shell to use when opening a new session.",
    }
    honor_ps1: HonorPS1 {
        type: bool,
        default: false,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "terminal.input.honor_ps1",
        description: "Whether to use your shell's PS1 prompt instead of the Warp prompt.",
    },
    saved_prompt: SavedPrompt {
        type: PromptSelection,
        default: PromptSelection::default(),
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    should_add_agent_mode_chip: ShouldAddAgentModeChip {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
    should_confirm_close_session: ShouldConfirmCloseSession {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "general.should_confirm_close_session",
        description: "Whether to show a confirmation dialog when closing a session.",
    },
    // Value is saved here but not shown in ui (can't be toggled in settings)
    should_confirm_shared_session_edit_access: ShouldConfirmSharedSessionEditAccess {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: true,
    }
    notifications: Notifications {
        type: NotificationsSettings,
        default: NotificationsSettings::default(),
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "notifications.preferences",
        max_table_depth: 1,
        description: "Notification preferences for terminal events.",
    }
    // This is a legacy setting that we no longer allow users to toggle after
    // context chips were introduced. We keep it only to respect users who
    // had previously disabled the dirty files chip via this setting.
    git_prompt_dirty_indicator: LegacyGitPromptDirtyIndicator {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: true,
        storage_key: "GitPromptDirtyIndicator",
    },
    // TODO: Remove this setting when `FeatureFlag::ProfilesDesignRevamp` is cleaned up.
    // When ProfilesDesignRevamp is enabled, model selectors are always shown in the prompt.
    // This setting only controls visibility when ProfilesDesignRevamp is disabled.
    show_model_selectors_in_prompt: ShowModelSelectorsInPrompt {
        type: bool,
        default: true,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Globally(RespectUserSyncSetting::Yes),
        surface: settings::SettingSurfaces::GUI,
        private: false,
        toml_path: "agents.warp_agent.input.show_model_selectors_in_prompt",
        description: "Whether to show AI model selectors in the input prompt.",
    },
    // Tracks whether the `gh` CLI is installed and authenticated on this machine.
    // Not synced because `gh` CLI availability is machine-specific.
    github_pr_chip_default_validation: GithubPrChipDefaultValidation {
        type: GithubPrPromptChipDefaultValidation,
        default: GithubPrPromptChipDefaultValidation::Unvalidated,
        supported_platforms: SupportedPlatforms::ALL,
        sync_to_cloud: SyncToCloud::Never,
        surface: settings::SettingSurfaces::GUI,
        private: true,
    },
]);

settings::macros::implement_setting_for_enum!(
    WorkingDirectoryConfig,
    SessionSettings,
    SupportedPlatforms::ALL,
    SyncToCloud::Never,
    surface: settings::SettingSurfaces::GUI,
    private: false,
    toml_path: "session.working_directory_config",
    max_table_depth: 1,
    description: "Controls the working directory used when opening new sessions.",
);

impl SessionSettings {
    pub fn default_session_mode(&self) -> DefaultSessionMode {
        let mode = *self.default_session_mode_internal.value();
        match mode {
            // Terminal and TabConfig don't require AI.
            DefaultSessionMode::Terminal | DefaultSessionMode::TabConfig => mode,
            DefaultSessionMode::Agent => DefaultSessionMode::Terminal,
            // DockerSandbox is gated on its feature flag; fall back to Terminal
            // when disabled so a stale stored value doesn't wedge the user.
            DefaultSessionMode::DockerSandbox => {
                if !crate::ChannelState::is_windows_warp_ish()
                    && warp_core::features::FeatureFlag::LocalDockerSandbox.is_enabled()
                {
                    mode
                } else {
                    DefaultSessionMode::Terminal
                }
            }
        }
    }

    /// Returns the stored default tab config path (only meaningful when mode is `TabConfig`).
    pub fn default_tab_config_path(&self) -> &str {
        &self.default_tab_config_path
    }

    /// Looks up the `TabConfig` matching the stored `default_tab_config_path`.
    /// Returns `None` if the path is empty or no loaded config matches.
    pub fn resolved_default_tab_config(
        &self,
        app: &AppContext,
    ) -> Option<crate::tab_configs::TabConfig> {
        let path_str = self.default_tab_config_path.as_str();
        if path_str.is_empty() {
            return None;
        }
        let path = std::path::Path::new(path_str);
        crate::user_config::WarpConfig::as_ref(app)
            .tab_configs()
            .iter()
            .find(|config| config.source_path.as_deref().is_some_and(|p| p == path))
            .cloned()
    }
}

#[cfg(test)]
#[path = "session_settings_cli_tests.rs"]
mod cli_tests;
