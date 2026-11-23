use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use enum_iterator::{Sequence, cardinality};
#[cfg(feature = "test-util")]
pub use overrides::{get_overrides, set_overrides};

#[derive(Copy, Clone, Hash, PartialEq, Eq, Debug, Sequence)]
pub enum FeatureFlag {
    Changelog,
    DebugMode,
    Autoupdate,
    WithSandboxTelemetry,

    WelcomeTips,
    ThinStrokes,
    WelcomeBlock,
    KnowledgeSidebar,

    RuntimeFeatureFlags,

    /// Enables cloud object related features for an explicit allowlist of team testers.
    CloudObjects,

    /// If `true`, fetch updated Warp channel versions from the Warp server endpoint instead of
    /// from GCP directly.
    FetchChannelVersionsFromWarpServer,

    /// Does grid storage go forwards or backwards
    SequentialStorage,

    /// If set, generators are executed in-band in all SSH sessions.
    InBandGeneratorsForSSH,

    /// If set, generators are executed using cmd.exe on Windows.
    RunGeneratorsWithCmdExe,

    /// Gates a bindable keyboard action for accepting command corrections.
    CommandCorrectionKey,

    /// If `true`, the "Show Initialization Block" menu item is added to the Blocks menu in the Mac
    /// menu bar.
    ToggleBootstrapBlock,



    /// Enabling context chips functionality for prompt
    ContextChips,

    /// Ligature Support in the Editor and Grid
    Ligatures,

    /// When enabled, the `History` rule from the command_corrections crate
    /// will be enabled. When the `History` rule is enabled, the command_corrections
    /// lib will use the user's history as a last-ditch effort to find a reasonable correction.
    CommandCorrectionsHistoryRule,

    /// Used to gate an experiment we're doing on WarpDev ONLY
    /// to get a sense of PTY throughput over time.
    RecordPtyThroughput,

    /// Whether to fetch generic string objects from the server.
    FetchGenericStringObjects,

    /// Enables a setting on Intel Dual-GPU Macs to enable use of the integrated GPU over the
    /// discrete GPU.
    IntegratedGPU,


    /// Whether the user is part of the Warp Alpha Program (AI Trusted Testers).
    /// This is enabled automatically for local and dev builds.
    /// Collect conversation and input autodetection data for agent mode.
    /// Also collects block data for Next Command, if enabled.
    AgentModeAnalytics,

    /// A setting to enable a traditional completions experience.
    ClassicCompletions,

    /// Force enable classic completions.
    ForceClassicCompletions,

    /// If enabled, autosuggestions are hidden when the tab completions
    /// menu is open (except when using completions-as-you-type).
    RemoveAutosuggestionDuringTabCompletions,

    /// Feature flag for cursor reflow fix (fixes part of the Alacritty resizing logic).
    ResizeFix,

    /// Enable multiselect in Notebooks and Warp Text.
    RichTextMultiselect,


    /// Makes the input editor's prompt selectable.
    SelectablePrompt,

    /// Enables the settings file feature.
    SettingsFile,

    /// Enables rect selection.
    RectSelection,

    /// Adds Alacritty as a supported terminal to import settings from.
    AlacrittySettingsImport,

    /// Enable dynamic enum parameter types for workflow arguments
    DynamicWorkflowEnums,

    /// Enables next action prediction within Warp, powered by AI.
    AgentPredict,




    /// Enables the shell selector, allowing us to open a new tab in
    /// a shell other than the default shell.
    ShellSelector,


    /// Lazily builds scenes at render time instead of eagerly when a view
    /// changes.
    LazySceneBuilding,


    /// Enables the full-screen "zen mode" setting, where we hide the tab bar if there's only one
    /// tab.
    FullScreenZenMode,

    /// Playground for reducing Warp UI clutter.
    MinimalistUI,

    /// Enables support for using native shell completions to supplement our
    /// completion specs.
    NativeShellCompletions,

    /// Adds avatar to the tab bar.
    AvatarInTabBar,

    /// Adds aliases for executing Warp Drive workflows.
    WorkflowAliases,

    SshDragAndDrop,
    DragTabsToWindows,

    /// Enables the overflow menu on AI blocks.
    AIBlockOverflowMenu,

    /// Enables cycling through the next command suggestions with down arrow.
    CycleNextCommandSuggestion,

    /// Enables multi-workspace selection.
    MultiWorkspace,

    /// Maximizes data in flat storage to reduce memory usage.
    MaximizeFlatStorage,

    /// Recognizes the OSC 8 hyperlink escape sequence and makes the
    /// linked text Cmd+click-able.
    OscHyperlinks,

    ImeMarkedText,

    /// Enables partial next command suggestions with a prefix.
    PartialNextCommandSuggestions,

    AIGeneratedOnboardingSuggestions,

    /// Enables iTerm image rendering
    ITermImages,

    /// Enables validation of autosuggestions.
    ValidateAutosuggestions,


    /// Enables using `esc` to clear autosuggestions.
    ClearAutosuggestionOnEscape,

    /// If enabled, the default theme is set to Adeberry for new users.
    DefaultAdeberryTheme,

    /// New, less intrusive autoupdate UI.
    AutoupdateUIRevamp,

    /// Enables Kitty image rendering
    KittyImages,

    /// Enables support for Warp Packs.
    WarpPacks,



    /// Enables auto-generated AI memories.
    AIMemories,




    /// Enables suggested rules.
    SuggestedRules,


    /// Forces users to login.
    ForceLogin,

    /// If enabled, command palette searches will use Tantivy search instead of the default fuzzy search.
    UseTantivySearch,


    /// MCP server v0 functionality.
    McpServer,


    /// UNIX shells running "natively" on Windows via MSYS2.
    MSYS2Shells,





    /// Enables reading images with the `read_files` tool.
    ReadImageFiles,








    /// Enables inline review comments on specific lines of code.
    ContextLineReviewComments,


    /// Remembers the per-conversation fast-forward state across local session restoration.
    RememberFastForwardState,

    /// Enables the find/replace in code editor
    CodeFindReplace,

    /// Enables file search functionality in command palette
    CommandPaletteFileSearch,


    /// Enables sending stderr warnings in FileGlobV2 results.
    FileGlobV2Warnings,



    /// Expands code diff edits to replace the current pane instead of opening in a new tab.
    ExpandEditToPane,


    /// Enables close button on left side of tabs
    TabCloseButtonOnLeft,

    /// Enables AI agent profile settings UI and functionality.
    ///
    /// TODO: When cleaning up this flag, also remove the `show_model_selectors_in_prompt`
    /// setting in [`SessionSettings`] (defined in `app/src/terminal/session_settings.rs`),
    /// as model selectors are always shown when this flag is enabled.
    ProfilesDesignRevamp,

    /// Enables new Search Codebase UI
    SearchCodebaseUI,

    /// Enables us to render linked code blocks
    LinkedCodeBlocks,

    /// Enables the tabbed file viewer
    TabbedEditorView,

    /// Enables multiple agent profiles in settings for managing different AI agent configurations.
    MultiProfile,

    /// Enables displaying imported PR review comments in the blocklist.
    PRCommentsV2,

    /// Gates the bundled skill-based implementation of PR comment fetching.
    PRCommentsSkill,

    /// A new first-time user experience which prioritizes choosing a coding repository.
    GetStartedTab,

    /// Enables Projects and Project management
    Projects,

    /// A context chip that shows when the PWD is inside of a git repository.
    CodeModeChip,

    /// Enables the prompt chip that displays the GitHub PR for the current branch.
    GithubPrPromptChip,

    /// A button on the homepage for easily creating new projects.
    CreateProjectFlow,

    /// Enables vim keybindings in the code editor.
    VimCodeEditor,

    /// Allows opening file links using the $EDITOR environment variable.
    AllowOpeningFileLinksUsingEditorEnv,

    /// Enables the ability to undo closed panes.
    UndoClosedPanes,

    /// Enables revert button for diff hunks in the gutter.
    RevertDiffHunk,

    /// Enables saving code review pane changes
    CodeReviewSaveChanges,

    /// Enables the file tree (with an entrypoint through code mode).
    FileTree,

    /// Enables ignoring input suggestions.
    AllowIgnoringInputSuggestions,

    /// Enables the one-time modal on app startup for existing users for the Code launch.



    /// Enables file- and diff set-level comments in the code review header.
    FileAndDiffSetComments,

    /// Enables discarding per-file and discarding all changes
    DiscardPerFileAndAllChanges,

    /// Enables UI zoom support (scaling the entire UI by a given percentage).
    UIZoom,


    /// Enables find/search in code review pane
    CodeReviewFind,

    /// Enables asynchronous find in terminal, running search on a background thread.
    AsyncFind,


    /// Enables auto-opening code review pane on first agent change and its setting UI.
    AutoOpenCodeReviewPane,


    /// Feature flags for the Build Plan Auto Reload experiment.
    BuildPlanAutoReloadBannerToggle,
    BuildPlanAutoReloadPostPurchaseModal,

    /// Enables inline code review functionality
    InlineCodeReview,

    /// Enables the /create-environment slash command for setting up Warp Environments
    CreateEnvironmentSlashCommand,

    /// Enables the local docker sandbox entrypoints in the client.
    LocalDockerSandbox,



    /// Enables the web search UI (when the model executes a web search).
    WebSearchUI,

    /// Enables the web fetch UI (when the model fetches content from URLs).
    WebFetchUI,


    /// Enables rendering of images in markdown files and AI responses.
    MarkdownImages,
    /// Enables rendering Mermaid diagrams in markdown notebooks.
    MarkdownMermaid,
    /// Enables editable Mermaid diagrams to behave atomically in notebook and plan editors.
    EditableMarkdownMermaid,

    /// Enables rendering markdown tables in notebooks.
    MarkdownTables,

    /// Renders `.ipynb` (Jupyter) files as a formatted, read-only notebook in
    /// Warp's notebook viewer instead of showing the raw JSON in the code editor.
    JupyterNotebookRendering,

    /// Enables rendering markdown tables inline in AI block list responses.
    BlocklistMarkdownTableRendering,
    /// Enables rendering markdown images inline in AI block list responses.
    BlocklistMarkdownImages,

    /// Enables the /fork-from slash command.
    ForkFromCommand,



    /// Enables global search
    GlobalSearch,

    /// Enables embedded code review comments.
    EmbeddedCodeReviewComments,

    /// Enables the revert to checkpoints feature.
    RevertToCheckpoints,

    /// Enables the /rewind slash command.
    RewindSlashCommand,



    /// Enables the inline history menu for quickly accessing previous commands and conversations.
    InlineHistoryMenu,



    /// Enables host selection in cloud mode.
    CloudModeHostSelector,

    /// Enables support for AM file diffs backed by the V4A patch format.
    V4AFileDiffs,

    /// Enables loading conversations in the Agent Management View.
    InteractiveConversationManagementView,





    /// Enables video recording of computer-use sessions for cloud agents.
    VideoRecording,

    /// Gates the Windows `gdigrab` recorder behind its own switch, on top of
    /// [`FeatureFlag::VideoRecording`]. Windows capture is newer and less proven than the
    /// macOS/Linux ffmpeg paths, so it rolls out and can be killed independently of them.
    WindowsVideoRecording,





    /// Enables configuring header toolbar item order, side placement, and visibility.
    ConfigurableToolbar,


    // Enables a side panel conversation list view for AgentView mode.


    /// Enables pluggable notifications via OSC 9 and OSC 777 escape sequences.
    /// External programs can trigger system and in-app notifications.
    PluggableNotifications,

    /// Dev-only: simulate a GitHub-unauthed user in the Environments page flow.
    ///
    /// This is intended for developer testing and should have no effect in release builds.
    SimulateGithubUnauthed,



    /// When enabled, we expose LSP as a tool to the agent
    LSPAsATool,








    /// Enables the OpenWarp launch modal announcing Warp going open-source.
    /// When enabled, the HOA onboarding flow is suppressed.
    OpenWarpLaunchModal,

    /// Updated tab styling (background colors, border, close button positioning, margins).
    NewTabStyling,





    /// Enables incremental (diff-based) buffer updates for auto-reload instead of full replace.
    IncrementalAutoReload,

    /// Enables scroll position preservation in the code review pane when file
    /// content changes via auto-reload.
    CodeReviewScrollPreservation,


    /// Gates the client-side multi-level orchestration surfaces: child
    /// conversations auto-executing their own `run_agents` calls and the
    /// confirmation-card disclosure that launched agents may start
    /// children of their own. When disabled, a child's `run_agents` call
    /// fails gracefully instead of presenting a card in a hidden pane.
    MultiLevelOrchestration,




    /// Enables Kitty keyboard protocol support (CSI u encoding, progressive enhancement).
    KittyKeyboardProtocol,

    /// Detects the word "figma" in the terminal input in real-time and shows a
    /// contextual button above the input.
    FigmaDetection,

    /// Enables header rows on all inline menus (label, tabs, resize handle).
    InlineMenuHeaders,

    /// Enables associating a tab color with a directory so tabs automatically
    /// adopt the configured color when their working directory matches.
    DirectoryTabColors,

    /// Enables vertical tab layout as an alternative to the horizontal tab bar.
    VerticalTabs,





    /// Enables tab configs — user-definable TOML templates for launching custom tab layouts.
    TabConfigs,

    /// Enables Warp local control through the standalone warpctrl CLI.
    WarpControlCli,



    /// Replaces the in-block warpification banner with a warpify footer.
    WarpifyFooter,



    /// Enables commit, push, and create-PR actions in the code review panel.
    GitOperationsInCodeReview,


    /// Trims trailing blank rows from CLI agent block output so unused vertical
    /// space is not rendered while the agent is running.
    TrimTrailingBlankLines,


    /// Enables summary mode in vertical tabs, showing condensed tab summaries
    /// instead of individual pane rows.
    VerticalTabsSummaryMode,




    /// Enables on-demand Factory repository checkouts and their credential scope.
    FactoryDeferredRepositories,
    /// Gates the driver behavior that writes GitHub credentials to disk
    /// (`~/.git-credentials`, `~/.config/gh/hosts.yaml`) and runs the
    /// background refresh loop that keeps them fresh during a task run.
    GitCredentialRefresh,


    /// Replaces the raw harness CLI command with a styled header showing CLI name + status icon.
    HarnessSessionHeader,

    /// Enables the code review view for remote sessions.
    RemoteCodeReview,

    /// Gates the Grouped Tabs feature.
    GroupedTabs,

    /// Gates the Pinned Tabs feature, which lets users pin individual tabs
    /// and whole tab groups so they stay at the front of the tab list and
    /// are protected from reordering.
    PinnedTabs,


    /// Gates NLD input classification matching the buffer against agent
    /// prompt history (in addition to shell command history). Still in
    /// development; currently disabled on all channels as a mitigation for
    /// misclassification bug reports (see PR #12586). Re-enable via
    /// `DOGFOOD_FLAGS` once the underlying issues are resolved.
    NldPromptHistoryMatch,

    /// Gates the custom model router feature, which allows users to define
    /// their own model routers.

    /// Enables state-mutating recovery for abnormal terminal lifecycle sequences.
    TerminalLifecycleRecovery,


    /// Shows a warning in the agent view when the active conversation's
    /// provider-side prompt cache has expired.
    PromptCacheExpiryWarning,


    /// Renders MCP tool-call request and response JSON as an interactive
    /// collapsible tree with typed colors and per-row Copy JSON, instead of
    /// a flat pretty-printed blob.
    McpJsonTreeView,

    /// Renders supported solid box-drawing characters (`U+2500..=U+257F`)
    /// procedurally as cell-filling rectangles instead of from the font,
    /// eliminating seams between adjacent box-drawing cells in the terminal.
    BoxDrawingGlyphs,






    /// Enables periodic workspace-handoff checkpoints during a cloud agent run,
    /// rather than only uploading a workspace snapshot once at end-of-run.
    /// Requires `OzHandoff` to also be enabled; a no-op for local runs and when
    /// `--no-snapshot` is set.
    PeriodicHandoffCheckpoints,

    /// Uses fzf or atuin for history search instead of Warp's command search.
    ShellWidgetHandoff,

    /// Attaches process-tree liveness signals to long-running command
    /// snapshots, giving the agent evidence that a silent command is still
    /// doing work before it decides to cancel.
    LrcActivitySignal,

    /// Gates Ctrl+R / Command Search history ranking on match quality and usage priors (recency,
    /// session, exit status) plus whitespace space-AND tokenization, instead of Skim's raw
    /// fuzzy-match score against the whole query as a single pattern. Disabling this is a full
    /// return to the pre-APP-5650 history search behavior, not an approximation of it.
    HistorySearchRankingV2,

    /// Advertises client support for server-issued task-message updates that
    /// replace inline computer-use screenshot bytes with references to
    /// Warp-managed object storage.
    StoredScreenshots,

    /// Lets cloud agent environment checkouts build repositories from bare git
    /// mirrors kept in the persistent build cache instead of cloning each one
    /// from the remote. Disabling it restores direct, network-only checkouts.
    GitMirrorCache,

    /// Resolves skill reference files in a Factory definition's skill folders during cloud runs,
    /// loading the selected skills from other repositories.
    FactorySkillReferences,
}

static FLAG_STATES: [AtomicBool; cardinality::<FeatureFlag>()] =
    [const { AtomicBool::new(false) }; { cardinality::<FeatureFlag>() }];

/// This map is populated by UserPreferences, which take precedence
/// over the global feature flag state.
static USER_PREFERENCE_MAP: [AtomicTriState; cardinality::<FeatureFlag>()] =
    [const { AtomicTriState::new() }; { cardinality::<FeatureFlag>() }];

/// Flag for whether or not feature flags have been globally initialized. Outside
/// of tests, this ensures that feature flags are only used after they're set
/// up by the app's `run_internal` function.
#[cfg(debug_assertions)]
static FEATURES_INITIALIZED: AtomicBool = AtomicBool::new(false);

/// Features used in debugging.
pub const DEBUG_FLAGS: &[FeatureFlag] = &[FeatureFlag::DebugMode, FeatureFlag::RuntimeFeatureFlags];
/// Features enabled only for the WarpLocal developer build.
pub const LOCAL_FLAGS: &[FeatureFlag] = &[];

/// Features enabled for the development team.  The expectation is that, over
/// time, these will move on to PREVIEW_FLAGS before being launched.
pub const DOGFOOD_FLAGS: &[FeatureFlag] = &[
    FeatureFlag::ToggleBootstrapBlock,
    FeatureFlag::RemoveAutosuggestionDuringTabCompletions,
    FeatureFlag::ResizeFix,
    FeatureFlag::AgentModeAnalytics,
    FeatureFlag::LazySceneBuilding,
    FeatureFlag::SshDragAndDrop,
    FeatureFlag::MultiWorkspace,
    FeatureFlag::ImeMarkedText,
    FeatureFlag::MSYS2Shells,
    FeatureFlag::ContextLineReviewComments,
    FeatureFlag::RunGeneratorsWithCmdExe,
    FeatureFlag::Projects,
    FeatureFlag::MarkdownImages,
    FeatureFlag::FileAndDiffSetComments,
    FeatureFlag::FileGlobV2Warnings,
    FeatureFlag::VideoRecording,
    FeatureFlag::WindowsVideoRecording,
    // End manually enabled Code features.
    FeatureFlag::EditableMarkdownMermaid,
    FeatureFlag::CodeReviewScrollPreservation,
    FeatureFlag::RememberFastForwardState,
    FeatureFlag::LocalDockerSandbox,
    FeatureFlag::WarpControlCli,
    FeatureFlag::TerminalLifecycleRecovery,
    FeatureFlag::PromptCacheExpiryWarning,
    FeatureFlag::JupyterNotebookRendering,
    FeatureFlag::MultiLevelOrchestration,
    FeatureFlag::McpJsonTreeView,
    FeatureFlag::BoxDrawingGlyphs,
    FeatureFlag::LrcActivitySignal,
    FeatureFlag::StoredScreenshots,
    FeatureFlag::GitMirrorCache,
    FeatureFlag::FactoryDeferredRepositories,
    FeatureFlag::FactorySkillReferences,
];

/// Features enabled for feature preview build users (e.g.: Friends of Warp).
/// All PREVIEW_FLAGS are also automatically added to dogfood builds (WarpDev).
pub const PREVIEW_FLAGS: &[FeatureFlag] = &[];

/// Features enabled for all release builds (i.e.: everything but WarpLocal).
/// NOTE: if you are promoting a feature from Preview to launch, you'll likely
/// want to enable the feature by default in app/Cargo.toml, rather than add it to RELEASE_FLAGS.
pub const RELEASE_FLAGS: &[FeatureFlag] = &[
    FeatureFlag::Autoupdate,
    FeatureFlag::Changelog,
    FeatureFlag::PeriodicHandoffCheckpoints,
    FeatureFlag::VideoRecording,
    FeatureFlag::WindowsVideoRecording,
    FeatureFlag::ImeMarkedText,
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    FeatureFlag::DragTabsToWindows,
];

/// Flags that we want to allow to switch at runtime (assuming RuntimeFeatureFlags is set)
pub const RUNTIME_FEATURE_FLAGS: &[FeatureFlag] = &[];

impl FeatureFlag {
    pub fn is_enabled(&self) -> bool {
        #[cfg(all(debug_assertions, not(feature = "test-util")))]
        {
            use std::sync::atomic::Ordering;
            assert!(
                FEATURES_INITIALIZED.load(Ordering::Relaxed),
                "Tried to check FeatureFlag::{self:?} before feature flags were initialized"
            );
        }

        overrides::get_override(*self)
            .or(USER_PREFERENCE_MAP[*self as usize].get())
            .or(Some(FLAG_STATES[*self as usize].load(Ordering::Relaxed)))
            .unwrap_or(false)
    }

    #[allow(dead_code)]
    pub fn set_enabled(self, enabled: bool) {
        // Allow calling this in integration tests because we sometimes use it in the app
        // during flows that integration tests cover.
        if cfg!(test) && cfg!(not(feature = "integration_tests")) {
            panic!(
                "Tried to globally enable {self:?} in a test. Use FeatureFlag::{self:?}.override_enabled instead"
            );
        }
        FLAG_STATES[self as usize].store(enabled, Ordering::Relaxed);
    }

    /// Sets a user preference for this flag. User preferences take precedence
    /// over the global feature flag state, and can be used to allow explicit opt-in
    /// and explicit opt-out behavior.
    pub fn set_user_preference(self, enabled: bool) {
        USER_PREFERENCE_MAP[self as usize].set(enabled);
    }

    /// Sets a thread-local test override for this flag. The override lasts
    /// until the returned guard is dropped.
    ///
    /// **Warning**: overrides do not work for tests of multi-threaded code. If
    /// you need to test multi-threaded code that's behind a feature flag, you'll
    /// need to set an override in _each_ thread.
    ///
    /// Tests should create overrides early on and allow them to be
    /// dropped automatically when they finish. This keeps overrides scoped to
    /// the duration of the test, since Rust doesn't have test lifecycle hooks.
    #[cfg(feature = "test-util")]
    pub fn override_enabled(self, enabled: bool) -> overrides::OverrideGuard {
        overrides::override_flag(self, enabled)
    }

    pub fn flag_description(&self) -> Option<&'static str> {
        use FeatureFlag::*;

        // Note: many feature flags are purposefully omitted from this list, in order to avoid blowing up
        // the Preview changelog. Features below which are enabled for Preview via PREVIEW_FLAGS, will be added to the changelog.
        // Features which are added to Stable should ideally have their feature flag removed entirely, but at the
        // very least, the feature flag should be removed from the Preview changelog by removing it from PREVIEW_FLAGS.
        // ** ONLY Preview-exclusive features should be added to this list! **
        match self {
            CodeReviewFind => Some("Enables the find bar in the code review pane."),
            BlocklistMarkdownImages => {
                Some("Enables rendering markdown images inline in AI block list responses.")
            }
            CreateEnvironmentSlashCommand => Some(
                "Enables the /create environment slash command for setting up Warp Environments with custom configurations.",
            ),
            GlobalSearch => Some("Enables global search in the left panel"),
            BlocklistMarkdownTableRendering => {
                Some("Enables rendering markdown tables inline in AI block list responses.")
            }
            MarkdownTables => {
                Some("Enables rendering and interaction support for markdown tables in notebooks.")
            }
            SettingsFile => Some(
                "Enables configuring Warp via a user-editable `settings.toml` file, with hot reload and error reporting for invalid values.",
            ),
            GitOperationsInCodeReview => Some(
                "Enables commit, push, and create-PR actions directly from the code review panel.",
            ),
            _ => None,
        }
    }
}

/// Marks that feature flags have been globally initialized.
pub fn mark_initialized() {
    #[cfg(debug_assertions)]
    FEATURES_INITIALIZED.store(true, std::sync::atomic::Ordering::Relaxed);
}

#[cfg(not(feature = "test-util"))]
mod overrides {
    #[inline(always)]
    pub fn get_override(_flag: super::FeatureFlag) -> Option<bool> {
        None
    }
}

/// Thread-local feature flag overrides for unit tests. For isolation, tests
/// should use overrides instead of globally modifying flags with [`super::FeatureFlag::set_enabled`].
#[cfg(feature = "test-util")]
mod overrides {
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::FeatureFlag;

    thread_local! {
        static FLAG_OVERRIDES: RefCell<HashMap<FeatureFlag,bool>> = RefCell::new(HashMap::new());
    }

    /// RAII guard to set feature flag overrides in tests. When the guard is
    /// dropped, it reverts to the global flag state.
    #[must_use = "if unused the override will be immediately cleared"]
    pub struct OverrideGuard {
        flag: FeatureFlag,
    }

    /// Gets the overridden state for a flag, if set.
    pub fn get_override(flag: FeatureFlag) -> Option<bool> {
        FLAG_OVERRIDES.with(|overrides| overrides.borrow().get(&flag).copied())
    }

    /// Gets the set of overridden flags.
    pub fn get_overrides() -> HashMap<FeatureFlag, bool> {
        FLAG_OVERRIDES.with(|overrides| overrides.borrow().clone())
    }

    /// Applies a set of overrides.
    ///
    /// This is intended to be used with [`get_overrides`] to apply a set of
    /// existing overrides to a newly-spawned thread.  If you are trying to
    /// override a single feature flag, use [`FeatureFlag::override_enabled`]
    /// instead.
    pub fn set_overrides(new_overrides: HashMap<FeatureFlag, bool>) {
        FLAG_OVERRIDES.with(|overrides| *overrides.borrow_mut() = new_overrides);
    }

    /// Set a thread-local override for a flag.
    pub fn override_flag(flag: FeatureFlag, enabled: bool) -> OverrideGuard {
        set_override(flag, enabled);
        OverrideGuard { flag }
    }

    fn set_override(flag: FeatureFlag, enabled: bool) {
        FLAG_OVERRIDES.with(|overrides| {
            let previous = overrides.borrow_mut().insert(flag, enabled);
            // We could support nested overrides, but it requires some care around
            // out-of-order drops - if overrides are set and then cleared out of
            // order, what should the state after each drop be?
            if previous.is_some() {
                panic!("Multiple overrides set for {flag:?}");
            }
        });
    }

    fn clear_override(flag: FeatureFlag) {
        FLAG_OVERRIDES.with(|overrides| {
            let previous = overrides.borrow_mut().remove(&flag);
            if previous.is_none() {
                panic!("Cleared override for {flag:?}, but none was set");
            }
        });
    }

    impl Drop for OverrideGuard {
        fn drop(&mut self) {
            clear_override(self.flag);
        }
    }
}

/// An atomic tri-state value.
///
/// This is initially unset, and can be set to a true or false value.
///
/// Writes and reads use [`Ordering::Relaxed`], so should not be used for
/// synchronization.
struct AtomicTriState(AtomicU8);

impl AtomicTriState {
    const fn new() -> Self {
        Self(AtomicU8::new(TriState::Unset as u8))
    }

    fn get(&self) -> Option<bool> {
        TriState::from(self.0.load(Ordering::Relaxed)).into()
    }

    fn set(&self, value: bool) {
        self.0.store(TriState::from(value) as u8, Ordering::Relaxed);
    }
}

/// A simple enum representing a tristate, to be used as the backing type
/// for [`AtomicTriState`].
enum TriState {
    Unset = 0,
    False = 1,
    True = 2,
}

impl From<bool> for TriState {
    fn from(value: bool) -> Self {
        if value {
            TriState::True
        } else {
            TriState::False
        }
    }
}

impl From<u8> for TriState {
    fn from(value: u8) -> Self {
        match value {
            0 => TriState::Unset,
            1 => TriState::False,
            2 => TriState::True,
            _ => unreachable!(),
        }
    }
}

impl From<TriState> for Option<bool> {
    fn from(value: TriState) -> Self {
        match value {
            TriState::Unset => None,
            TriState::False => Some(false),
            TriState::True => Some(true),
        }
    }
}

#[cfg(test)]
#[path = "features_tests.rs"]
mod tests;
