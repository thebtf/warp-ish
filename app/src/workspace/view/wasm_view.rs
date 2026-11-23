//! WASM-only view functions for the Workspace.

use warp_core::channel::ChannelState;
use warpui::elements::{ChildView, Element};
use warpui::{AppContext, SingletonEntity, ViewContext, ViewHandle};

use super::PanelPosition;
use crate::terminal::TerminalView;
use crate::ui_components::icons;
use crate::wasm_nux_dialog::{WasmNUXDialog, WasmNUXDialogEvent};
use crate::workspace::action::WorkspaceAction;
use crate::workspace::view::Workspace;

impl Workspace {
    pub(super) fn build_wasm_nux_dialog(ctx: &mut ViewContext<Self>) -> ViewHandle<WasmNUXDialog> {
        let wasm_nux_dialog = ctx.add_typed_action_view(|_| WasmNUXDialog::new());
        ctx.subscribe_to_view(&wasm_nux_dialog, |me, _, event, ctx| match event {
            WasmNUXDialogEvent::Close => {
                me.show_wasm_nux_dialog = false;
                ctx.notify();
            }
        });
        wasm_nux_dialog
    }

}
