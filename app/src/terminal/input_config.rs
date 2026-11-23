//! Input kind and explicit lock state shared by terminal editor consumers.

use serde::{Deserialize, Serialize};

use super::input_type::InputType;

/// Configuration for the terminal pane's input.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct InputConfig {
    /// The type of the terminal input.
    pub input_type: InputType,

    /// If `true`, we will not attempt to auto-detect the best input type.
    pub is_locked: bool,
}

impl InputConfig {
    pub fn with_toggled_type(self) -> Self {
        let input_type = if self.input_type.is_ai() {
            InputType::Shell
        } else {
            InputType::AI
        };
        Self { input_type, ..self }
    }

    pub fn with_shell_type(self) -> Self {
        Self {
            input_type: InputType::Shell,
            ..self
        }
    }

    pub fn with_input_type(self, input_type: InputType) -> Self {
        Self { input_type, ..self }
    }

    pub fn locked(self) -> Self {
        Self {
            is_locked: true,
            ..self
        }
    }

    pub fn is_ai(&self) -> bool {
        self.input_type == InputType::AI
    }

    pub fn is_shell(&self) -> bool {
        self.input_type == InputType::Shell
    }
}

#[cfg(test)]
#[path = "input_config_tests.rs"]
mod tests;
