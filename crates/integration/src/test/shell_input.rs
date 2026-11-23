//! Integration coverage for plain terminal input.

use warp::integration_testing::input::input_contains_string;
use warp::integration_testing::step::new_step_with_default_assertions;
use warp::integration_testing::terminal::wait_until_bootstrapped_single_pane_for_tab;

use super::{Builder, new_builder};

pub fn test_shell_input_preserves_literal_slash_and_newline() -> Builder {
    new_builder()
        .with_step(wait_until_bootstrapped_single_pane_for_tab(0))
        .with_step(
            new_step_with_default_assertions("Keep a literal slash when adding a line to shell input")
                .with_typed_characters(&["/"])
                .with_keystrokes(&["shift-enter"])
                .add_assertion(input_contains_string(0, "/\n".to_owned())),
        )
}
