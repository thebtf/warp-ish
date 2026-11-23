use super::{
    AgentConversationData, AgentConversationSummary, ChargedUsageTotals, ConversationUsageMetadata,
};

/// Conversations persisted before the provider cost was dropped still carry it; the blob must
/// keep deserializing and the stale key must not be written back.
#[test]
fn conversation_usage_metadata_ignores_a_persisted_provider_cost() {
    let metadata: ConversationUsageMetadata = serde_json::from_str(
        r#"{"was_summarized":false,"context_window_usage":0.0,"credits_spent":1.5,"total_provider_cost_in_cents":3.2}"#,
    )
    .unwrap();

    assert_eq!(metadata.credits_spent, 1.5);
    assert_eq!(metadata.total_billed_cost_in_cents, None);
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("total_provider_cost_in_cents")
    );
}

#[test]
fn conversation_usage_metadata_defaults_missing_billed_cost_to_unknown() {
    let metadata: ConversationUsageMetadata = serde_json::from_str(
        r#"{"was_summarized":false,"context_window_usage":0.0,"credits_spent":0.0}"#,
    )
    .unwrap();

    assert_eq!(metadata.total_billed_cost_in_cents, None);
    assert_eq!(metadata.billed_cost_in_cents(), None);
    assert!(
        !serde_json::to_string(&metadata)
            .unwrap()
            .contains("total_billed_cost_in_cents")
    );
}

#[test]
fn billed_cost_prefers_summed_charges_over_graphql_total() {
    let metadata = ConversationUsageMetadata {
        total_billed_cost_in_cents: Some(12.0),
        total_charged_usage: Some(ChargedUsageTotals {
            input_cost_in_cents: 3.0,
            output_cost_in_cents: 4.0,
            platform_cost_in_cents: 1.0,
            web_search_cost_in_cents: 0.5,
            ..Default::default()
        }),
        ..Default::default()
    };

    assert_eq!(metadata.billed_cost_in_cents(), Some(8.5));
}

#[test]
fn billed_cost_falls_back_to_graphql_total_without_summed_charges() {
    let metadata = ConversationUsageMetadata {
        total_billed_cost_in_cents: Some(12.0),
        ..Default::default()
    };

    assert_eq!(metadata.billed_cost_in_cents(), Some(12.0));
}

#[test]
fn charged_usage_totals_add_assign_sums_web_search_fields() {
    let mut a = ChargedUsageTotals {
        web_search_count: 2,
        web_search_cost_in_cents: 4.0,
        ..Default::default()
    };
    let b = ChargedUsageTotals {
        web_search_count: 3,
        web_search_cost_in_cents: 6.0,
        ..Default::default()
    };

    a += b;

    assert_eq!(a.web_search_count, 5);
    assert!((a.web_search_cost_in_cents - 10.0).abs() < 1e-6);
}

#[test]
fn charged_usage_totals_deserializes_legacy_payload_without_web_search_fields() {
    let totals: ChargedUsageTotals = serde_json::from_str(
        r#"{"input_cost_in_cents":1.0,"output_cost_in_cents":2.0,"input_cache_read_cost_in_cents":0.0,"input_cache_write_cost_in_cents":0.0,"platform_cost_in_cents":0.0,"input_tokens":10,"output_tokens":5,"input_cache_read_tokens":0,"input_cache_write_tokens":0}"#,
    )
    .unwrap();

    assert_eq!(totals.web_search_count, 0);
    assert_eq!(totals.web_search_cost_in_cents, 0.0);
    assert_eq!(totals.total_cost_in_credits(), 0.0);
    assert!((totals.total_cost_in_cents() - 3.0).abs() < 1e-6);
}

#[test]
fn summary_roundtrips_through_json() {
    let summary = AgentConversationSummary {
        initial_query: "Initial query".to_owned(),
        title: "Root title".to_owned(),
        initial_working_directory: Some("/tmp/repo".to_owned()),
        is_restorable: true,
        is_unlisted_auto_code_diff: false,
    };
    let json = serde_json::to_string(&summary).expect("summary should serialize");
    let roundtripped: AgentConversationSummary =
        serde_json::from_str(&json).expect("summary should deserialize");
    assert_eq!(roundtripped, summary);
}

#[test]
fn agent_conversation_data_roundtrips_last_event_sequence() {
    let data = AgentConversationData {
        orchestration_harness_type: Some("claude".to_string()),
        last_event_sequence: Some(42),
        ..Default::default()
    };
    let json = serde_json::to_string(&data).expect("serialize");
    let roundtripped: AgentConversationData = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(roundtripped.last_event_sequence, Some(42));
    assert_eq!(
        roundtripped.orchestration_harness_type.as_deref(),
        Some("claude")
    );
}

#[test]
fn agent_conversation_data_accepts_legacy_orchestration_avatar_id() {
    let legacy_json = r#"{"orchestration_avatar_id":"orbit"}"#;
    let data: AgentConversationData =
        serde_json::from_str(legacy_json).expect("legacy rows must deserialize");

    assert_eq!(data.orchestration_harness_type.as_deref(), Some("orbit"));
}

#[test]
fn agent_conversation_data_roundtrips_remote_child_marker() {
    let data = AgentConversationData {
        is_remote_child: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&data).expect("serialize");
    let roundtripped: AgentConversationData = serde_json::from_str(&json).expect("deserialize");
    assert!(roundtripped.is_remote_child);
}

#[test]
fn agent_conversation_data_roundtrips_optimistic_root_marker() {
    let data = AgentConversationData {
        root_task_is_optimistic: Some(true),
        ..Default::default()
    };
    let json = serde_json::to_string(&data).expect("serialize");
    let roundtripped: AgentConversationData = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(roundtripped.root_task_is_optimistic, Some(true));
}

#[test]
fn agent_conversation_data_deserializes_legacy_payload_without_last_event_sequence() {
    // Legacy rows persisted before this feature landed omit the field
    // entirely. `#[serde(default)]` must accept them as `None`.
    let legacy_json = r#"{"server_conversation_token":null}"#;
    let data: AgentConversationData =
        serde_json::from_str(legacy_json).expect("legacy rows must deserialize");
    assert_eq!(data.last_event_sequence, None);
    assert_eq!(data.orchestration_harness_type, None);
    assert!(!data.is_remote_child);
}

#[test]
fn agent_conversation_data_skips_serializing_none_last_event_sequence() {
    let data = AgentConversationData::default();
    let json = serde_json::to_string(&data).expect("serialize");
    assert!(
        !json.contains("last_event_sequence"),
        "None should be skipped in serialized output: {json}"
    );
}

#[test]
fn agent_conversation_data_roundtrips_use_warp_credits_instead_of_chatgpt() {
    let data = AgentConversationData {
        use_warp_credits_instead_of_chatgpt: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&data).expect("serialize");
    let roundtripped: AgentConversationData = serde_json::from_str(&json).expect("deserialize");
    assert!(roundtripped.use_warp_credits_instead_of_chatgpt);

    let legacy_json = r#"{"server_conversation_token":null}"#;
    let legacy: AgentConversationData =
        serde_json::from_str(legacy_json).expect("legacy rows must deserialize");
    assert!(!legacy.use_warp_credits_instead_of_chatgpt);
}

#[test]
fn agent_conversation_data_roundtrips_pinned() {
    let data = AgentConversationData {
        pinned: true,
        ..Default::default()
    };
    let json = serde_json::to_string(&data).expect("serialize");
    let roundtripped: AgentConversationData = serde_json::from_str(&json).expect("deserialize");
    assert!(roundtripped.pinned);
}

#[test]
fn agent_conversation_data_skips_serializing_unpinned() {
    let data = AgentConversationData::default();
    let json = serde_json::to_string(&data).expect("serialize");
    assert!(
        !json.contains("pinned"),
        "Unpinned default should be skipped: {json}"
    );
}

#[test]
fn agent_conversation_data_legacy_rows_default_to_unpinned() {
    let legacy_json = r#"{"server_conversation_token":null}"#;
    let data: AgentConversationData =
        serde_json::from_str(legacy_json).expect("legacy rows must deserialize");
    assert!(!data.pinned);
}

#[test]
fn charged_usage_totals_saturates_cumulative_totals() {
    let mut totals = ChargedUsageTotals {
        input_tokens: u32::MAX,
        output_tokens: u32::MAX,
        input_cache_read_tokens: u32::MAX,
        input_cache_write_tokens: 1,
        ..Default::default()
    };
    assert_eq!(totals.total_tokens(), u32::MAX);

    totals += totals;
    assert_eq!(totals.input_tokens, u32::MAX);
    assert_eq!(totals.output_tokens, u32::MAX);
    assert_eq!(totals.input_cache_read_tokens, u32::MAX);
    assert_eq!(totals.input_cache_write_tokens, 2);
    assert_eq!(totals.total_tokens(), u32::MAX);
}
