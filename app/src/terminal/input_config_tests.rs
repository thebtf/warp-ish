use super::{InputConfig, InputType};

#[test]
fn legacy_input_config_serde_fields_and_tags_roundtrip() {
    let shell = InputConfig {
        input_type: InputType::Shell,
        is_locked: true,
    };
    let ai = InputConfig {
        input_type: InputType::AI,
        is_locked: false,
    };
    assert_eq!(
        serde_json::to_value(shell).unwrap(),
        serde_json::json!({"input_type":"Shell","is_locked":true})
    );
    assert_eq!(
        serde_json::to_value(ai).unwrap(),
        serde_json::json!({"input_type":"AI","is_locked":false})
    );
    assert_eq!(
        serde_json::from_value::<InputConfig>(
            serde_json::json!({"input_type":"Shell","is_locked":true})
        )
        .unwrap(),
        shell
    );
    assert_eq!(
        serde_json::from_value::<InputConfig>(
            serde_json::json!({"input_type":"AI","is_locked":false})
        )
        .unwrap(),
        ai
    );
}

#[test]
fn value_edits_preserve_the_independent_input_field() {
    let shell_locked = InputConfig {
        input_type: InputType::Shell,
        is_locked: true,
    };
    let ai_unlocked = InputConfig {
        input_type: InputType::AI,
        is_locked: false,
    };
    assert_eq!(
        shell_locked.with_toggled_type(),
        InputConfig {
            input_type: InputType::AI,
            is_locked: true
        }
    );
    assert_eq!(
        ai_unlocked.with_toggled_type(),
        InputConfig {
            input_type: InputType::Shell,
            is_locked: false
        }
    );
    assert_eq!(
        ai_unlocked.with_shell_type(),
        InputConfig {
            input_type: InputType::Shell,
            is_locked: false
        }
    );
    assert_eq!(
        shell_locked.with_input_type(InputType::AI),
        InputConfig {
            input_type: InputType::AI,
            is_locked: true
        }
    );
    assert_eq!(
        ai_unlocked.locked(),
        InputConfig {
            input_type: InputType::AI,
            is_locked: true
        }
    );
    assert!(shell_locked.is_shell());
    assert!(!shell_locked.is_ai());
    assert!(ai_unlocked.is_ai());
    assert!(!ai_unlocked.is_shell());
}
