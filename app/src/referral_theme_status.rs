use serde::Deserialize;
use warp_core::user_preferences::GetUserPreferences as _;
use warpui::{Entity, ModelContext};

// These keys and their formats are part of the existing local preferences.
const SENT_REFERRAL_THEME_KEY: &str = "ReferralThemeActive";
const RECEIVED_REFERRAL_THEME_KEY: &str = "ReceivedReferralTheme";

/// Availability of previously unlocked themes from local preferences.
pub struct ReferralThemeStatus {
    sent_referral_theme: bool,
    received_referral_theme: bool,
}

impl Entity for ReferralThemeStatus {
    type Event = ();
}

impl ReferralThemeStatus {
    pub fn new(ctx: &mut ModelContext<Self>) -> Self {
        let preferences = ctx.private_user_preferences();
        let sent = preferences.read_value(SENT_REFERRAL_THEME_KEY).unwrap_or_default();
        let received = preferences.read_value(RECEIVED_REFERRAL_THEME_KEY).unwrap_or_default();
        Self {
            sent_referral_theme: sent_theme_unlocked(sent.as_deref()),
            received_referral_theme: received_theme_unlocked(received.as_deref()),
        }
    }

    pub fn sent_referral_theme_active(&self) -> bool {
        self.sent_referral_theme
    }

    pub fn received_referral_theme_active(&self) -> bool {
        self.received_referral_theme
    }
}

#[derive(Deserialize)]
enum ReferralThemeFetchStatus {
    NotFetched,
    Inactive,
    Active,
}

fn sent_theme_unlocked(value: Option<&str>) -> bool {
    value.and_then(|value| value.parse::<bool>().ok()).unwrap_or(false)
}

fn received_theme_unlocked(value: Option<&str>) -> bool {
    matches!(
        value.and_then(|value| serde_json::from_str(value).ok()),
        Some(ReferralThemeFetchStatus::Active)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_existing_theme_unlock_preferences() {
        assert!(sent_theme_unlocked(Some("true")));
        assert!(!sent_theme_unlocked(Some("false")));
        assert!(!sent_theme_unlocked(None));
        assert!(received_theme_unlocked(Some(r#""Active""#)));
        assert!(!received_theme_unlocked(Some(r#""Inactive""#)));
        assert!(!received_theme_unlocked(Some(r#""NotFetched""#)));
        assert!(!received_theme_unlocked(Some("malformed")));
        assert!(!received_theme_unlocked(None));
    }
}
