use settings::schema::SettingSchemaEntry;

#[test]
fn retired_cloud_conversation_storage_is_not_registered() {
    assert!(
        !inventory::iter::<SettingSchemaEntry>
            .into_iter()
            .any(|entry| matches!(
                entry.storage_key,
                "cloud_conversation_storage_enabled" | "CloudConversationStorageEnabled"
            ))
    );
}

#[test]
fn crash_reporting_is_not_a_public_setting() {
    assert!(
        !inventory::iter::<SettingSchemaEntry>
            .into_iter()
            .any(|entry| {
                entry.hierarchy == Some("privacy") && entry.storage_key == "crash_reporting_enabled"
            })
    );
}

#[test]
fn analytics_consent_is_not_a_registered_setting() {
    assert!(
        !inventory::iter::<SettingSchemaEntry>
            .into_iter()
            .any(|entry| {
                entry.hierarchy == Some("privacy") && entry.storage_key == "telemetry_enabled"
            })
    );
    assert!(
        !inventory::iter::<SettingSchemaEntry>
            .into_iter()
            .any(|entry| {
                matches!(
                    entry.storage_key,
                    "telemetry_banner_dismissed" | "TelemetryBannerDismissed"
                )
            })
    );
    assert!(
        inventory::iter::<SettingSchemaEntry>
            .into_iter()
            .any(|entry| {
                entry.hierarchy == Some("privacy")
                    && entry.storage_key == "custom_secret_regex_list"
            })
    );
}
