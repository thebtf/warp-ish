//! Extension trait for accessing the private user preferences backend.
//!
//! Public settings are accessed exclusively through the settings macros
//! (`define_settings_group!`) and the `settings::PublicPreferences` wrapper,
//! which restricts direct access at compile time.

use std::ops::Deref;

use settings::PrivatePreferences;
use uuid::Uuid;
use warpui_core::SingletonEntity;
use warpui_extras::user_preferences::UserPreferences;

/// An extension trait on [`warpui_core::AppContext`] for accessing private user
/// preferences.
///
/// Private settings are always stored in the platform-native store (e.g.
/// UserDefaults on macOS, registry on Windows, JSON file on Linux) and never
/// appear in the user-visible settings file.
pub trait GetUserPreferences {
    /// Returns the preferences backend for private settings.
    ///
    /// Private settings are always stored in the platform-native store and never
    /// appear in the user-visible settings file.
    fn private_user_preferences(&self) -> &dyn UserPreferences;
}

impl GetUserPreferences for warpui_core::AppContext {
    fn private_user_preferences(&self) -> &dyn UserPreferences {
        <PrivatePreferences as SingletonEntity>::as_ref(self).deref()
    }
}

/// Key used to persist the anonymous id to user defaults. We use "ExperimentId" as the key
/// since we use the persisted id to determine experiment groups, and we want to avoid
/// associating it with telemetry.
const ANONYMOUS_ID_KEY: &str = "ExperimentId";

/// Reads the persisted anonymous id from user defaults, if it exists and is a
/// valid uuid.
fn get_persisted_anonymous_id(ctx: &dyn GetUserPreferences) -> Option<Uuid> {
    let anonymous_id = ctx
        .private_user_preferences()
        .read_value(ANONYMOUS_ID_KEY)
        .unwrap_or_default()?;
    match Uuid::parse_str(&anonymous_id) {
        Ok(uuid) => Some(uuid),
        Err(e) => {
            log::warn!("Error parsing persisted anonymous id from user defaults: {e:?}");
            None
        }
    }
}

/// Gets the persisted anonymous id if possible, otherwise generates a new uuid
/// and saves it to user defaults.
pub fn get_or_create_anonymous_id(ctx: &dyn GetUserPreferences) -> Uuid {
    get_persisted_anonymous_id(ctx).unwrap_or_else(|| {
        let uuid = Uuid::new_v4();
        let _ = ctx
            .private_user_preferences()
            .write_value(ANONYMOUS_ID_KEY, uuid.to_string());
        uuid
    })
}
