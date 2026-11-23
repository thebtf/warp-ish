use std::borrow::Cow;

use serde::{Deserialize, Serialize};

use crate::AppId;

#[derive(Debug, Deserialize, Serialize)]
pub struct ChannelConfig {
    /// The application ID for this channel.
    pub app_id: AppId,

    /// The name of the file to which logs should be written.
    pub logfile_name: Cow<'static, str>,

    /// Configuration for talking to Warp's servers.
    pub server_config: WarpServerConfig,
    /// Configuration for Oz/ambient agents.
    pub oz_config: OzConfig,
    /// Configuration for autoupdate functionality.
    pub autoupdate_config: Option<AutoupdateConfig>,
    /// Configuration for statically-bundled MCP OAuth credentials.
    pub mcp_static_config: Option<McpStaticConfig>,
}

/// Configuration for GCP Identity-Aware Proxy authentication, present only on staging builds.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct IapConfig {
    /// The IAP OAuth2 client ID used as the audience for identity tokens.
    pub audiences: Cow<'static, str>,
    /// The service account email to impersonate when acquiring IAP credentials.
    pub service_account_email: Cow<'static, str>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct WarpServerConfig {
    /// The root URL for the standard server pool.
    pub server_root_url: Cow<'static, str>,
    /// The pre-warmed sandbox control origin, defaulting to the server root URL.
    #[serde(default)]
    pub prewarmed_sandbox_control_url: Option<Cow<'static, str>>,
    /// The URL for the RTC server, which serves real-time updates for Warp Drive objects.
    pub rtc_server_url: Cow<'static, str>,
    /// The URL for the session sharing server, or [`None`] if session sharing is not
    /// supported.
    pub session_sharing_server_url: Option<Cow<'static, str>>,
    /// Configuration for GCP Identity-Aware Proxy authentication, present only on
    /// staging builds. [`None`] on production builds.
    #[serde(default)]
    pub iap_config: Option<IapConfig>,
}

impl WarpServerConfig {
    pub fn production() -> Self {
        Self {
            server_root_url: "https://app.warp.dev".into(),
            prewarmed_sandbox_control_url: None,
            rtc_server_url: "wss://rtc.app.warp.dev/graphql/v2".into(),
            session_sharing_server_url: Some("wss://sessions.app.warp.dev".into()),
            iap_config: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct OzConfig {
    /// Root URL for the Oz (ambient agent management) dashboard.
    pub oz_root_url: Cow<'static, str>,

    /// URL to use as the audience when issuing workload identity tokens. If [`None`], falls back
    /// to [`WarpServerConfig::server_root_url`]. This exists so the audience is not overridden
    /// when a custom server root URL is provided (e.g. an ngrok URL for local development).
    pub workload_audience_url: Option<Cow<'static, str>>,
}

impl OzConfig {
    pub fn production() -> Self {
        Self {
            oz_root_url: "https://oz.warp.dev".into(),
            workload_audience_url: None,
        }
    }
}

#[derive(Debug, Deserialize, Serialize)]
pub struct AutoupdateConfig {
    /// The base URL for fetching autoupdate versions and updated release bundles.
    pub releases_base_url: Cow<'static, str>,
    /// Whether or not to display menu items relating to autoupdate.
    pub show_autoupdate_menu_items: bool,
}

/// Configuration for statically-bundled MCP OAuth credentials.
///
/// These are credentials for OAuth providers where dynamic client registration
/// is not supported and we instead ship pre-registered client IDs and secrets.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct McpStaticConfig {
    /// Per-provider OAuth credentials.
    pub providers: Vec<McpOAuthProviderConfig>,
}

/// A single OAuth provider's credentials for MCP authentication.
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct McpOAuthProviderConfig {
    /// The issuer URL of the OAuth provider (e.g. `https://github.com/login/oauth`).
    pub issuer: Cow<'static, str>,
    /// The OAuth client ID registered for this channel.
    pub client_id: Cow<'static, str>,
    /// The OAuth client secret registered for this channel.
    pub client_secret: Cow<'static, str>,
    /// A separately registered native client that permits loopback redirects.
    /// This must never be inferred from the custom-scheme client.
    #[serde(default)]
    pub loopback_client: Option<McpOAuthLoopbackClientConfig>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct McpOAuthLoopbackClientConfig {
    pub client_id: Cow<'static, str>,
    /// Native OAuth clients should normally be public, but retain optional
    /// secret support for providers that require one.
    #[serde(default)]
    pub client_secret: Option<Cow<'static, str>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_telemetry_config_is_ignored_without_changing_local_configuration() {
        let config = ChannelConfig {
            app_id: AppId::new("dev", "warpish", "WarpIsh"),
            logfile_name: "warp-ish.log".into(),
            server_config: WarpServerConfig::production(),
            oz_config: OzConfig::production(),
            autoupdate_config: Some(AutoupdateConfig {
                releases_base_url: "https://updates.example.invalid".into(),
                show_autoupdate_menu_items: true,
            }),
            mcp_static_config: None,
        };
        let expected = serde_json::to_value(&config).unwrap();
        assert!(expected.get("telemetry_config").is_none());
        let mut legacy = expected.clone();
        legacy["telemetry_config"] = serde_json::json!({
            "telemetry_file_name": "retired-telemetry.json",
            "rudderstack_config": {
                "write_key": "retired-non-ugc-key",
                "root_url": "https://collector.example.invalid",
                "ugc_write_key": "retired-ugc-key"
            }
        });

        let legacy_json = serde_json::to_string(&legacy).unwrap();
        let restored: ChannelConfig = serde_json::from_str(&legacy_json).unwrap();
        assert_eq!(restored.app_id, config.app_id);
        assert_eq!(restored.logfile_name, "warp-ish.log");
        assert_eq!(serde_json::to_value(restored).unwrap(), expected);
    }
}
