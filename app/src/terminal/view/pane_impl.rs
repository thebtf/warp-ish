//! This module contains the implementation of `BackingView` for `TerminalView`, as well as
//! business logic for integrating the terminal view with the pane infra (`crate::pane_group`).
use warpui::elements::{
    ConstrainedBox, CrossAxisAlignment, Empty, Flex, MainAxisAlignment, MainAxisSize,
    ParentElement, Shrinkable,
};
use warpui::prelude::Container;
use warpui::text_layout::ClipConfig;
use warpui::ui_components::components::UiComponent;
use warpui::{
    AppContext, Element, ModelHandle, SingletonEntity, TypedActionView, ViewContext,
    WeakModelHandle,
};

use super::{Event, PaneConfiguration, TerminalAction, TerminalViewState};
use crate::appearance::Appearance;
use crate::menu::{MenuItem, MenuItemFields};
use crate::pane_group::focus_state::{PaneFocusHandle, PaneGroupFocusEvent, PaneGroupFocusState};
use crate::pane_group::pane::view::PaneHeaderAction;
use crate::pane_group::pane::view::header::components::{
    CenteredHeaderEdgeWidth, header_edge_min_width, render_pane_header_buttons,
    render_pane_header_title_text, render_three_column_header,
};
use crate::pane_group::pane::view::header::render_pane_header_draggable;
use crate::pane_group::pane::{PaneStack, view};
use crate::pane_group::{BackingView, SplitPaneState, TOGGLE_MAXIMIZE_PANE_BINDING_NAME};
use crate::terminal::{TerminalManager, TerminalView};
use crate::ui_components::buttons::icon_button_with_color;
use crate::ui_components::{blended_colors, icons};
use crate::util::bindings::keybinding_name_to_display_string;

impl TerminalView {
    /// Returns a reference to the focus handle if one has been set.
    pub fn focus_handle(&self) -> Option<&PaneFocusHandle> {
        self.focus_handle.as_ref()
    }

    fn handle_focus_state_event(
        &mut self,
        _focus_state: ModelHandle<PaneGroupFocusState>,
        event: &PaneGroupFocusEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        let Some(focus_handle) = &self.focus_handle else {
            return;
        };

        if focus_handle.is_affected(event) {
            self.on_pane_state_change(ctx);
        }
    }

    /// Set the pane configuration for this terminal view.
    pub fn set_pane_configuration(&mut self, pane_configuration: ModelHandle<PaneConfiguration>) {
        self.pane_configuration = pane_configuration;
    }

    /// Respond to changes to the active session or split pane states.
    pub fn on_pane_state_change(&mut self, ctx: &mut ViewContext<Self>) {
        self.refresh_pane_header(ctx);

        // Trigger refresh of the pane header overflow menu to reflect the new pane state
        // (e.g., updating the Maximize/Minimize pane menu item)
        self.pane_configuration.update(ctx, |config, ctx| {
            config.refresh_pane_header_overflow_menu_items(ctx);
        });

        if !self.is_pane_focused(ctx) {
            // Don't need to call ctx.notify here as clear_selected_blocks already
            // calls ctx.notify internally
            self.clear_selected_blocks(ctx);
            self.clear_selected_text(ctx);
        } else {
            ctx.notify();
        }
    }

    pub fn refresh_pane_header(&mut self, ctx: &mut ViewContext<Self>) {
        let is_active_session = self.is_active_session(ctx);
        self.pane_configuration
            .update(ctx, move |pane_config, ctx| {
                pane_config.set_show_active_pane_indicator(is_active_session, ctx);
                pane_config.refresh_pane_header_overflow_menu_items(ctx);
            });
    }

    pub(super) fn update_pane_configuration(&mut self, ctx: &mut ViewContext<Self>) {
        let new_pane_title = self.terminal_title.clone();
        self.pane_configuration.update(ctx, |pane_config, ctx| {
            pane_config.set_title(new_pane_title, ctx);
            pane_config.notify_header_content_changed(ctx);
        });
    }

    pub(super) fn is_pane_focused(&self, app: &AppContext) -> bool {
        self.focus_handle.as_ref().is_none_or(|h| h.is_focused(app))
    }

    pub fn is_active_session(&self, app: &AppContext) -> bool {
        self.focus_handle
            .as_ref()
            .is_some_and(|h| h.is_active_session(app))
    }

    pub(super) fn split_pane_state(&self, app: &AppContext) -> SplitPaneState {
        self.focus_handle
            .as_ref()
            .map_or(SplitPaneState::NotInSplitPane, |h| h.split_pane_state(app))
    }

    fn can_pop_pane_stack(&self, app: &AppContext) -> bool {
        self.pane_stack
            .as_ref()
            .and_then(|stack| stack.upgrade(app))
            .is_some_and(|stack| stack.as_ref(app).depth() > 1)
    }

    pub(super) fn pop_pane_stack(&mut self, ctx: &mut ViewContext<Self>) {
        if let Some(stack) = self
            .pane_stack
            .as_ref()
            .and_then(|stack| stack.upgrade(ctx))
        {
            stack.update(ctx, |stack, ctx| {
                if stack.depth() > 1 {
                    let _ = stack.pop(ctx);
                }
            });
        }
    }

    fn render_header_back_button(&self, app: &AppContext) -> (Box<dyn Element>, f32) {
        if warpui::platform::is_mobile_device() || !self.can_pop_pane_stack(app) {
            return (Empty::new().finish(), 0.);
        }

        let appearance = Appearance::as_ref(app);
        let theme = appearance.theme();
        let ui_builder = appearance.ui_builder().clone();
        let button = icon_button_with_color(
            appearance,
            icons::Icon::ArrowLeft,
            false,
            self.pane_stack_back_mouse_state.clone(),
            theme.sub_text_color(theme.background()).into(),
        )
        .with_tooltip(move || {
            ui_builder
                .tool_tip("Back to previous terminal".to_string())
                .build()
                .finish()
        })
        .build()
        .on_click(|ctx, _, _| {
            ctx.dispatch_typed_action::<PaneHeaderAction<TerminalAction, TerminalAction>>(
                PaneHeaderAction::CustomAction(TerminalAction::PopPaneStack),
            );
        })
        .finish();
        (button, header_edge_min_width(1))
    }

    fn render_header_title(
        &self,
        header_ctx: &view::HeaderRenderContext,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let appearance = Appearance::as_ref(app);
        let title = self.pane_configuration.as_ref(app).title().to_owned();
        let is_pane_dragging = header_ctx.draggable_state.is_dragging();
        let mut center_row = Flex::row()
            .with_main_axis_alignment(MainAxisAlignment::Center)
            .with_cross_axis_alignment(CrossAxisAlignment::Center)
            .with_main_axis_size(MainAxisSize::Min);
        if let Some(indicator) = self.render_terminal_mode_indicator(app) {
            center_row.add_child(Container::new(indicator).with_margin_right(4.).finish());
        }
        let title_text = render_pane_header_title_text(title, appearance, ClipConfig::start());
        if is_pane_dragging {
            // During drag, all children must be non-flex to avoid panics
            // from infinite constraints on flex children.
            center_row.add_child(title_text);
        } else {
            center_row.add_child(Shrinkable::new(1.0, title_text).finish());
        }

        center_row.finish()
    }

    /// Returns the right-column element and the estimated minimum width of
    /// the right-column content (used to set the edge width for centering).
    fn render_header_actions(
        &self,
        header_ctx: &view::HeaderRenderContext,
        app: &AppContext,
    ) -> (Box<dyn Element>, f32) {
        let appearance = Appearance::as_ref(app);
        let icon_color = Some(
            appearance
                .theme()
                .sub_text_color(appearance.theme().background()),
        );
        let show_close_button = self.split_pane_state(app).is_in_split_pane();
        let right = render_pane_header_buttons::<TerminalAction, TerminalAction>(
            header_ctx,
            appearance,
            show_close_button,
            icon_color,
            None,
        );
        let min_width =
            header_edge_min_width(show_close_button as u32 + header_ctx.has_overflow_items as u32);
        (right, min_width)
    }

    fn render_terminal_pane_header(
        &self,
        header_ctx: &view::HeaderRenderContext,
        app: &AppContext,
    ) -> Box<dyn Element> {
        let (left, min_back_width) = self.render_header_back_button(app);
        let center = self.render_header_title(header_ctx, app);
        let (right, min_actions_width) = self.render_header_actions(header_ctx, app);

        let header = render_three_column_header(
            left,
            center,
            right,
            CenteredHeaderEdgeWidth {
                min: min_actions_width.max(min_back_width),
                max: 200.0,
            },
            header_ctx.header_left_inset,
            header_ctx.draggable_state.is_dragging(),
        );
        render_pane_header_draggable::<TerminalView>(
            self.pane_configuration.clone(),
            header,
            header_ctx.draggable_state.clone(),
            app,
        )
    }
}

impl BackingView for TerminalView {
    type PaneHeaderOverflowMenuAction = TerminalAction;
    type CustomAction = TerminalAction;
    type AssociatedData = ModelHandle<Box<dyn TerminalManager>>;

    fn set_pane_stack(
        &mut self,
        pane_stack: WeakModelHandle<PaneStack<Self>>,
        _ctx: &mut ViewContext<Self>,
    ) {
        self.pane_stack = Some(pane_stack);
    }

    fn handle_pane_header_overflow_menu_action(
        &mut self,
        action: &Self::PaneHeaderOverflowMenuAction,
        ctx: &mut ViewContext<Self>,
    ) {
        self.handle_action(action, ctx);
    }

    fn handle_custom_action(&mut self, action: &Self::CustomAction, ctx: &mut ViewContext<Self>) {
        self.handle_action(action, ctx);
    }

    fn close(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.emit(Event::CloseRequested);
    }

    fn focus_contents(&mut self, ctx: &mut ViewContext<Self>) {
        self.redetermine_global_focus(ctx);
    }

    fn pane_header_overflow_menu_items(
        &self,
        ctx: &AppContext,
    ) -> Vec<MenuItem<Self::PaneHeaderOverflowMenuAction>> {
        let split_state = self.split_pane_state(ctx);
        if !split_state.is_in_split_pane() {
            return Vec::new();
        }

        vec![
            MenuItemFields::toggle_pane_action(split_state.is_maximized())
                .with_on_select_action(TerminalAction::ToggleMaximizePane)
                .with_key_shortcut_label(keybinding_name_to_display_string(
                    TOGGLE_MAXIMIZE_PANE_BINDING_NAME,
                    ctx,
                ))
                .into_item(),
        ]
    }

    fn should_render_header(&self, app: &AppContext) -> bool {
        self.split_pane_state(app).is_in_split_pane() || self.can_pop_pane_stack(app)
    }

    fn render_header_content(
        &self,
        header_ctx: &view::HeaderRenderContext<'_>,
        app: &AppContext,
    ) -> view::HeaderContent {
        view::HeaderContent::Custom {
            element: self.render_terminal_pane_header(header_ctx, app),
            has_custom_draggable_behavior: true,
        }
    }

    /// Sets the focus handle for this terminal view, enabling it to track its split pane state.
    fn set_focus_handle(&mut self, focus_handle: PaneFocusHandle, ctx: &mut ViewContext<Self>) {
        self.focus_handle = Some(focus_handle.clone());
        // Subscribe to focus state changes to update pane state when focus/split state changes
        ctx.subscribe_to_model(
            focus_handle.focus_state_handle(),
            Self::handle_focus_state_event,
        );
        self.input.update(ctx, |input, ctx| {
            input.set_focus_handle(focus_handle, ctx);
        });
        self.on_pane_state_change(ctx);
    }
}

impl TerminalView {
    /// Shows an error indicator when the terminal fails, or the active shell on Windows.
    fn render_terminal_mode_indicator(&self, app: &AppContext) -> Option<Box<dyn Element>> {
        let appearance = Appearance::as_ref(app);
        let font_size = appearance.ui_font_size();

        // Error indicator takes priority
        if matches!(self.current_state.state, TerminalViewState::Errored) {
            return Some(
                ConstrainedBox::new(
                    icons::Icon::AlertTriangle
                        .to_warpui_icon(appearance.theme().ui_error_color().into())
                        .finish(),
                )
                .with_height(font_size)
                .with_width(font_size)
                .finish(),
            );
        }

        // Shell indicator (Windows only)
        if let Some(shell_indicator_type) = self.shell_indicator_type {
            let shell_indicator_icon = shell_indicator_type
                .to_icon()
                .to_warpui_icon(
                    blended_colors::text_sub(appearance.theme(), appearance.theme().background())
                        .into(),
                )
                .finish();
            return Some(
                ConstrainedBox::new(shell_indicator_icon)
                    .with_height(font_size)
                    .with_width(font_size)
                    .finish(),
            );
        }

        None
    }
}
