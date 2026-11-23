//! Footer for enabling shell integration in detected SSH and subshell sessions.

mod warpify_footer;

use std::sync::Arc;

use parking_lot::FairMutex;
use pathfinder_color::ColorU;
use warp_core::ui::appearance::Appearance;
use warp_core::ui::color::contrast::{
    MinimumAllowedContrast, high_enough_contrast, pick_best_foreground_color,
};
use warp_core::ui::theme::Fill as ThemeFill;
use warp_core::ui::theme::color::internal_colors;
use warpui::elements::{ChildView, Empty, ParentElement};
use warpui::{AppContext, Element, Entity, ModelHandle, SingletonEntity, View, ViewContext, ViewHandle};

use self::warpify_footer::{WarpifyFooterView, WarpifyFooterViewEvent};
use super::{RichContentInsertionPosition, TerminalView};
use crate::settings::InputModeSettings;
use crate::terminal::TerminalModel;
use crate::terminal::model_events::{ModelEvent, ModelEventDispatcher};
use crate::ui_components::blended_colors;
use crate::view_components::action_button::ActionButtonTheme;

impl TerminalView {
    pub(super) fn register_subscriptions_for_use_agent_footer(
        &mut self,
        ctx: &mut ViewContext<Self>,
    ) {
        ctx.subscribe_to_view(&self.use_agent_footer, |me, _, event, ctx| {
            me.handle_use_agent_footer_event(event, ctx);
        });

        let input_mode_settings = InputModeSettings::handle(ctx);
        let mut was_pinned_to_top = input_mode_settings
            .as_ref(ctx)
            .input_mode
            .is_pinned_to_top();
        ctx.subscribe_to_model(&input_mode_settings, move |me, settings_handle, _, ctx| {
            let is_pinned_to_top = settings_handle.as_ref(ctx).is_pinned_to_top();
            if was_pinned_to_top != is_pinned_to_top {
                was_pinned_to_top = is_pinned_to_top;
                me.maybe_show_use_agent_footer_in_blocklist(ctx);
            }
        });
    }

    fn handle_use_agent_footer_event(
        &mut self,
        event: &UseAgentToolbarEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            UseAgentToolbarEvent::Dismiss => {
                self.hide_use_agent_footer_in_blocklist(ctx);
                ctx.notify();
            }
            UseAgentToolbarEvent::Warpify => {
                self.hide_use_agent_footer_in_blocklist(ctx);
                self.trigger_subshell_bootstrap(None, false, ctx);
            }
        }
    }

    pub(super) fn should_render_use_agent_footer(
        &self,
        _model: &TerminalModel,
        app: &AppContext,
    ) -> bool {
        self.use_agent_footer.as_ref(app).is_warpify_active(app)
    }

    pub(super) fn maybe_show_use_agent_footer_in_blocklist(&mut self, ctx: &mut ViewContext<Self>) {
        // This is a bit of a hack- but it ensures we never show more than one footer in the
        // blocklist.
        self.hide_use_agent_footer_in_blocklist(ctx);
        let (should_render_footer, is_alt_screen_active) = {
            let model = self.model.lock();
            (
                self.should_render_use_agent_footer(&model, ctx),
                model.is_alt_screen_active(),
            )
        };
        if is_alt_screen_active || !should_render_footer {
            return;
        }

        let should_insert_after_block = !InputModeSettings::as_ref(ctx).is_pinned_to_top();

        self.insert_rich_content(
            None,
            self.use_agent_footer.clone(),
            None,
            RichContentInsertionPosition::Append {
                insert_below_long_running_block: should_insert_after_block,
            },
            ctx,
        );
    }

    pub(super) fn hide_use_agent_footer_in_blocklist(&mut self, ctx: &mut ViewContext<Self>) {
        let mut model = self.model.lock();
        let block_list = model.block_list_mut();
        block_list.remove_rich_content(self.use_agent_footer.id());
        ctx.notify();
    }
}

/// Displays the Warpify prompt while the detected shell supports bootstrapping.
pub struct UseAgentToolbar {
    warpify_footer_view: ViewHandle<WarpifyFooterView>,
}

impl UseAgentToolbar {
    pub(crate) fn new(
        terminal_model: Arc<FairMutex<TerminalModel>>,
        model_event_dispatcher: &ModelHandle<ModelEventDispatcher>,
        ctx: &mut ViewContext<Self>,
    ) -> Self {
        let warpify_footer_view =
            ctx.add_typed_action_view(|ctx| WarpifyFooterView::new(terminal_model, ctx));

        ctx.subscribe_to_view(&warpify_footer_view, |me, _, event, ctx| {
            me.handle_warpify_footer_event(event, ctx);
        });

        ctx.subscribe_to_model(model_event_dispatcher, |me, _, event, ctx| {
            if let ModelEvent::TerminalModeSwapped(..) = event {
                me.notify_and_notify_children(ctx);
            }
        });

        Self { warpify_footer_view }
    }

    fn handle_warpify_footer_event(
        &mut self,
        event: &WarpifyFooterViewEvent,
        ctx: &mut ViewContext<Self>,
    ) {
        match event {
            WarpifyFooterViewEvent::Warpify => {
                ctx.emit(UseAgentToolbarEvent::Warpify);
            }
            WarpifyFooterViewEvent::Dismiss => {
                ctx.emit(UseAgentToolbarEvent::Dismiss);
            }
        }
    }

    pub(in crate::terminal) fn notify_and_notify_children(&mut self, ctx: &mut ViewContext<Self>) {
        ctx.notify();
        self.warpify_footer_view.update(ctx, |_, ctx| ctx.notify());
    }

    /// Activates the prompt for enabling shell integration in the subshell.
    pub(in crate::terminal) fn show_warpify(&mut self, ctx: &mut ViewContext<Self>) {
        self.warpify_footer_view.update(ctx, |view, ctx| {
            view.show(ctx);
        });
        ctx.notify();
    }

    /// Deactivates the prompt.
    pub(in crate::terminal) fn clear_warpify(&mut self, ctx: &mut ViewContext<Self>) {
        self.warpify_footer_view.update(ctx, |view, ctx| {
            view.clear(ctx);
        });
        ctx.notify();
    }

    /// Returns whether the prompt is currently active.
    pub(in crate::terminal) fn is_warpify_active(&self, app: &AppContext) -> bool {
        self.warpify_footer_view.as_ref(app).is_active()
    }
}

/// Actions reported by the Warpify footer.
pub enum UseAgentToolbarEvent {
    Dismiss,
    Warpify,
}

impl Entity for UseAgentToolbar {
    type Event = UseAgentToolbarEvent;
}

impl View for UseAgentToolbar {
    fn ui_name() -> &'static str {
        "UseAgentToolbar"
    }

    fn render(&self, app: &AppContext) -> Box<dyn Element> {
        if self.warpify_footer_view.as_ref(app).is_active() {
            return ChildView::new(&self.warpify_footer_view).finish();
        }

        Empty::new().finish()
    }
}

#[derive(Clone)]
pub(super) struct AgentFooterButtonTheme {
    /// When set, enables alt-screen contrast adjustment for text and border.
    terminal_model: Option<Arc<FairMutex<TerminalModel>>>,
}

impl AgentFooterButtonTheme {
    pub fn new(terminal_model: Option<Arc<FairMutex<TerminalModel>>>) -> Self {
        Self { terminal_model }
    }

    /// Returns the inferred background colour of the alt screen, if active.
    fn inferred_alt_screen_bg(&self) -> Option<ColorU> {
        let terminal_model = self.terminal_model.as_ref()?;
        let terminal_model = terminal_model.lock();
        terminal_model
            .is_alt_screen_active()
            .then(|| terminal_model.alt_screen().inferred_bg_color())
            .flatten()
    }

    /// Picks a colour that contrasts well against `bg`, choosing between two
    /// neutral candidates.
    fn contrast_adjusted_color(
        bg: ColorU,
        default: ColorU,
        contrast: MinimumAllowedContrast,
        appearance: &Appearance,
    ) -> ColorU {
        if high_enough_contrast(default, bg, contrast) {
            default
        } else {
            pick_best_foreground_color(
                bg,
                blended_colors::neutral_2(appearance.theme()),
                blended_colors::neutral_6(appearance.theme()),
                contrast,
            )
        }
    }
}

impl ActionButtonTheme for AgentFooterButtonTheme {
    fn background(&self, hovered: bool, appearance: &Appearance) -> Option<ThemeFill> {
        if hovered {
            Some(internal_colors::fg_overlay_2(appearance.theme()))
        } else {
            None
        }
    }

    fn border(&self, appearance: &Appearance) -> Option<ColorU> {
        let color = appearance.theme().outline().into_solid();
        if let Some(bg) = self.inferred_alt_screen_bg() {
            return Some(Self::contrast_adjusted_color(
                bg,
                color,
                MinimumAllowedContrast::NonText,
                appearance,
            ));
        }
        Some(color)
    }

    fn text_color(
        &self,
        _hovered: bool,
        _background: Option<ThemeFill>,
        appearance: &Appearance,
    ) -> ColorU {
        let color = appearance
            .theme()
            .sub_text_color(appearance.theme().surface_1())
            .into_solid();

        // If rendered in the alt screen, the footer is rendered with the inferred background color
        // of the alt screen output grid (if there is one). In such cases, we have to ensure that
        // the text within the footer is high-contrast enough to be legible, since the background
        // color can essentially be anything.
        if let Some(bg) = self.inferred_alt_screen_bg() {
            return Self::contrast_adjusted_color(
                bg,
                color,
                MinimumAllowedContrast::Text,
                appearance,
            );
        }
        color
    }
}

#[cfg(test)]
#[path = "mod_tests.rs"]
mod tests;
