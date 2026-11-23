use std::sync::{Arc, Mutex};

use ordered_float::OrderedFloat;
use pathfinder_geometry::vector::vec2f;
use settings::Setting as _;
use warpui::elements::Empty;
use warpui::platform::WindowStyle;
use warpui::units::IntoPixels;
use warpui::{App, ModelHandle};

use super::*;
use crate::editor::EditorView;
use crate::search::SyncDataSource;
use crate::search::data_source::{Query, QueryResult};
use crate::search::item::SearchItem;
use crate::search::mixer::DataSourceRunErrorWrapper;
use crate::settings::{InputModeSettings, InputSettings};
use crate::terminal::block_list_viewport::InputMode;
use crate::terminal::input::InputSuggestionsMode;
use crate::terminal::input::buffer_model::InputBufferModel;
use crate::test_util::settings::initialize_settings_for_tests;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TestAction(&'static str);
impl InlineMenuAction for TestAction {
    const MENU_TYPE: InlineMenuType = InlineMenuType::InlineHistoryMenu;
}

#[derive(Clone)]
struct TestItem {
    text: &'static str,
    score: f64,
    disabled: bool,
}
impl SearchItem for TestItem {
    type Action = TestAction;
    fn render_icon(&self, _: ItemHighlightState, _: &Appearance) -> Box<dyn Element> {
        Empty::new().finish()
    }
    fn render_item(&self, _: ItemHighlightState, _: &AppContext) -> Box<dyn Element> {
        Empty::new().finish()
    }
    fn score(&self) -> OrderedFloat<f64> {
        OrderedFloat(self.score)
    }
    fn accept_result(&self) -> TestAction {
        TestAction(self.text)
    }
    fn execute_result(&self) -> TestAction {
        self.accept_result()
    }
    fn accessibility_label(&self) -> String {
        self.text.into()
    }
    fn is_disabled(&self) -> bool {
        self.disabled
    }
}

struct TestSource(Arc<Mutex<Vec<TestItem>>>);
impl SyncDataSource for TestSource {
    type Action = TestAction;
    fn run_query(
        &self,
        _: &Query,
        _: &AppContext,
    ) -> Result<Vec<QueryResult<TestAction>>, DataSourceRunErrorWrapper> {
        Ok(self
            .0
            .lock()
            .unwrap()
            .iter()
            .cloned()
            .map(QueryResult::from)
            .collect())
    }
}

fn pane_size(height: f32) -> crate::terminal::SizeInfo {
    crate::terminal::SizeInfo::new(
        vec2f(800., height),
        8f32.into_pixels(),
        16f32.into_pixels(),
        0f32.into_pixels(),
        0f32.into_pixels(),
    )
}

fn fixture(
    app: &mut App,
) -> (
    ModelHandle<InputSuggestionsModeModel>,
    ModelHandle<InlineMenuPositioner>,
) {
    // Settings use mock preferences and no-op secure storage. No session/history is opened.
    initialize_settings_for_tests(app);
    app.add_singleton_model(|_| Appearance::mock());
    app.add_singleton_model(|_| crate::workspace::ToastStack);
    app.add_singleton_model(|_| crate::workspace::sync_inputs::SyncedInputState::mock());
    app.add_singleton_model(|_| crate::vim_registers::VimRegisters::new());
    app.add_singleton_model(|_| {
        crate::settings_view::keybindings::KeybindingChangedNotifier::mock()
    });

    let (window_id, editor) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
        EditorView::new(Default::default(), ctx)
    });
    let buffer = app.add_model(|ctx| InputBufferModel::new(&editor, ctx));
    let mode = app.add_model(|_| InputSuggestionsModeModel::new(buffer));
    let positioner = app.add_model(|ctx| {
        InlineMenuPositioner::new(
            &mode,
            "inline_test_terminal_content".into(),
            "inline_test_input".into(),
            pane_size(1000.),
            window_id,
            ctx,
        )
    });
    (mode, positioner)
}

fn reopen_history(mode: &ModelHandle<InputSuggestionsModeModel>, app: &mut App) {
    mode.update(app, |model, ctx| {
        model.set_mode(InputSuggestionsMode::Closed, ctx);
        model.set_mode(InputSuggestionsMode::InlineHistoryMenu, ctx);
    });
}

#[test]
fn positioner_uses_local_input_modes_caps_and_persisted_resize_without_agent_controller() {
    App::test((), |mut app| async move {
        let (mode, positioner) = fixture(&mut app);
        for (input_mode, below) in [
            (InputMode::PinnedToTop, true),
            (InputMode::PinnedToBottom, false),
            (InputMode::Waterfall, false),
        ] {
            InputModeSettings::handle(&app).update(&mut app, |settings, ctx| {
                settings.input_mode.set_value(input_mode, ctx).unwrap()
            });
            reopen_history(&mode, &mut app);
            positioner.read(&app, |positioner, ctx| {
                assert_eq!(positioner.should_render_inline_menu_below_input(), below);
                assert!(!positioner.should_render_results_in_reverse(ctx));
            });
        }
        InputModeSettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .input_mode
                .set_value(InputMode::PinnedToBottom, ctx)
                .unwrap()
        });
        reopen_history(&mode, &mut app);
        let before = positioner.read(&app, |positioner, ctx| positioner.inline_menu_height(ctx));
        positioner.update(&mut app, |positioner, ctx| {
            assert_eq!(
                positioner.apply_resize_delta(InlineMenuType::InlineHistoryMenu, -40., ctx),
                Some(40.)
            );
            positioner.persist_custom_content_heights(ctx);
        });
        app.read(|ctx| {
            assert_eq!(
                InputSettings::as_ref(ctx)
                    .inline_menu_custom_content_heights
                    .value()
                    .get(&InlineMenuType::InlineHistoryMenu),
                Some(&(before + 40.))
            )
        });
        positioner.update(&mut app, |positioner, ctx| {
            positioner.set_size_info(pane_size(100.), ctx)
        });
        let without_header = {
            let _headers = FeatureFlag::InlineMenuHeaders.override_enabled(false);
            positioner.read(&app, |positioner, ctx| positioner.inline_menu_height(ctx))
        };
        let with_header = {
            let _headers = FeatureFlag::InlineMenuHeaders.override_enabled(true);
            positioner.read(&app, |positioner, ctx| positioner.inline_menu_height(ctx))
        };
        assert!(with_header < without_header);
        assert!(without_header < 100.); // The real message-bar frame consumes space.
        positioner.update(&mut app, |positioner, ctx| {
            positioner.set_size_info(pane_size(1.), ctx)
        });
        InputModeSettings::handle(&app).update(&mut app, |settings, ctx| {
            settings
                .input_mode
                .set_value(InputMode::Waterfall, ctx)
                .unwrap()
        });
        reopen_history(&mode, &mut app);
        positioner.read(&app, |positioner, ctx| {
            assert_eq!(positioner.inline_menu_height(ctx), 0.);
            assert!(positioner.should_hide_inline_menu_for_pane_size(ctx));
            assert!(positioner.should_render_inline_menu_below_input());
            assert!(!positioner.should_render_results_in_reverse(ctx));
        });
        // Returning to a large pane restores the persisted user height, not the cap.
        positioner.update(&mut app, |positioner, ctx| {
            positioner.set_size_info(pane_size(1000.), ctx)
        });
        positioner.read(&app, |positioner, ctx| {
            assert_eq!(positioner.inline_menu_height(ctx), before + 40.)
        });
    });
}

#[test]
fn real_inline_view_refresh_mouse_selection_and_close_work_without_agent_controller() {
    App::test((), |mut app| async move {
        let (mode, positioner) = fixture(&mut app);
        let rows = Arc::new(Mutex::new(vec![
            TestItem {
                text: "first",
                score: 0.,
                disabled: false,
            },
            TestItem {
                text: "last enabled",
                score: 1.,
                disabled: false,
            },
            TestItem {
                text: "disabled",
                score: 2.,
                disabled: true,
            },
        ]));
        let mixer = app.add_model(|_| {
            let mut mixer = SearchMixer::new();
            mixer.add_sync_source(TestSource(rows.clone()), []);
            mixer
        });
        let (_, menu) = app.add_window(WindowStyle::NotStealFocus, |ctx| {
            InlineMenuView::new(mixer.clone(), positioner, &mode, ctx)
        });
        reopen_history(&mode, &mut app);
        let refresh = |app: &mut App| {
            mixer.update(app, |mixer, ctx| {
                mixer.run_query(
                    Query {
                        text: String::new(),
                        filters: Default::default(),
                    },
                    ctx,
                )
            })
        };
        refresh(&mut app);
        menu.read(&app, |menu, ctx| {
            assert_eq!(menu.result_count(), 3);
            assert_eq!(
                menu.model().as_ref(ctx).selected_item(),
                Some(&TestAction("last enabled"))
            );
        });
        menu.update(&mut app, |menu, ctx| {
            menu.handle_action(
                &InlineMenuRowAction::Select {
                    result_index: 0,
                    item: TestAction("first"),
                },
                ctx,
            )
        });
        menu.read(&app, |menu, ctx| {
            assert_eq!(
                menu.model().as_ref(ctx).selected_item(),
                Some(&TestAction("first"))
            )
        });
        rows.lock().unwrap().clear();
        refresh(&mut app);
        menu.read(&app, |menu, ctx| {
            assert_eq!(menu.result_count(), 0);
            assert_eq!(menu.model().as_ref(ctx).selected_item(), None);
        });
        rows.lock().unwrap().push(TestItem {
            text: "after refresh",
            score: 0.,
            disabled: false,
        });
        refresh(&mut app);
        menu.read(&app, |menu, ctx| {
            assert_eq!(
                menu.model().as_ref(ctx).selected_item(),
                Some(&TestAction("after refresh"))
            )
        });
        mode.update(&mut app, |mode, ctx| {
            mode.set_mode(InputSuggestionsMode::Closed, ctx)
        });
        menu.read(&app, |menu, ctx| {
            assert_eq!(menu.model().as_ref(ctx).selected_item(), None)
        });
    });
}
