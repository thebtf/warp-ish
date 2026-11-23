use warpui::App;

use super::*;
use crate::terminal::model::ansi::{BootstrappedValue, Handler as _, InitShellValue};
use crate::test_util::add_window_with_terminal;
use crate::test_util::terminal::initialize_app_for_terminal_view;

#[test]
fn warpify_footer_follows_subshell_prompt_visibility() {
    App::test((), |mut app| async move {
        initialize_app_for_terminal_view(&mut app);
        let terminal = add_window_with_terminal(&mut app, None);

        terminal.update(&mut app, |view, ctx| {
            {
                let mut model = view.model.lock();
                model.init_shell(InitShellValue {
                    session_id: 0.into(),
                    shell: "zsh".to_owned(),
                    ..Default::default()
                });
                model.bootstrapped(BootstrappedValue {
                    shell: "zsh".to_owned(),
                    ..Default::default()
                });
                model.simulate_long_running_block("ssh localhost", "Password:");
                assert!(!view.should_render_use_agent_footer(&model, ctx));
            }

            view.use_agent_footer.update(ctx, |footer, ctx| footer.show_warpify(ctx));
            view.maybe_show_use_agent_footer_in_blocklist(ctx);
            view.maybe_show_use_agent_footer_in_blocklist(ctx);
            let footer_id = view.use_agent_footer.id();
            let rendered_footer = |model: &TerminalModel| {
                let blocks = model.block_list();
                blocks
                    .last_non_hidden_rich_content_block_after_block(Some(blocks.active_block_index()))
                    .map(|(_, item)| item.view_id)
            };
            {
                let model = view.model.lock();
                assert!(view.should_render_use_agent_footer(&model, ctx));
                assert_eq!(rendered_footer(&model), Some(footer_id));
            }

            view.use_agent_footer.update(ctx, |footer, ctx| footer.clear_warpify(ctx));
            view.handle_use_agent_footer_event(&UseAgentToolbarEvent::Dismiss, ctx);
            {
                let model = view.model.lock();
                assert!(!view.should_render_use_agent_footer(&model, ctx));
                assert_ne!(rendered_footer(&model), Some(footer_id));
            }
        });
    })
}
