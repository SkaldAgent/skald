//! Runtime configuration, read from the environment (relay.md §7). Sensible
//! defaults so the relay boots with zero config in local dev.
//!
//! | Env var          | Meaning                                              | Default                       |
//! |------------------|------------------------------------------------------|-------------------------------|
//! | `RELAY_BIND`     | full `ip:port` to listen on                          | `0.0.0.0:8080`                |
//! | `PORT`           | port only (used if no RELAY_BIND)                    | —                             |
//! | `RELAY_DB`       | SQLite file path                                     | `data/relay.db`               |
//! | `APNS_KEY_PATH`  | (push-live) JSON file with team/key/PEM              | `./config/apns-key.json`      |
//! | `APNS_BUNDLE_ID` | (push-live) iOS bundle id (used as `apns-topic`)     | — (required when push-live)   |
//! | `APNS_SANDBOX`   | (push-live) `1`/`true` → api.sandbox.push.apple.com  | `0` (production)              |

use std::net::SocketAddr;

/// APNs configuration, populated from `config/apns-key.json` and env vars when
/// the `push-live` cargo feature is on. The PEM is already newline-decoded by
/// `serde_json` so it can be passed straight to `jsonwebtoken`.
#[cfg(feature = "push-live")]
#[derive(Debug, Clone)]
pub struct ApnsConfig {
    pub team_id: String,
    pub key_id: String,
    /// PEM-encoded PKCS#8 EC private key (P-256), with real newlines.
    pub private_key_pem: String,
    /// Bundle ID for the iOS app (used as `apns-topic`).
    pub bundle_id: String,
    /// If true, send to api.sandbox.push.apple.com.
    pub sandbox: bool,
}

#[derive(Debug, Clone)]
pub struct Config {
    pub bind: SocketAddr,
    pub db_path: String,
    /// `None` ⇒ the relay falls back to [`LogPusher`] (relay still boots).
    #[cfg(feature = "push-live")]
    pub apns: Option<ApnsConfig>,
}

impl Config {
    pub fn from_env() -> Config {
        let default_bind: SocketAddr = "0.0.0.0:8080".parse().unwrap();
        let bind = std::env::var("RELAY_BIND")
            .ok()
            .and_then(|s| s.parse().ok())
            .or_else(|| {
                std::env::var("PORT")
                    .ok()
                    .and_then(|p| format!("0.0.0.0:{p}").parse().ok())
            })
            .unwrap_or(default_bind);
        let db_path = std::env::var("RELAY_DB").unwrap_or_else(|_| "data/relay.db".into());
        Config {
            bind,
            db_path,
            #[cfg(feature = "push-live")]
            apns: ApnsConfig::load_from_env(),
        }
    }
}

#[cfg(feature = "push-live")]
impl ApnsConfig {
    /// Read `APNS_KEY_PATH` (default `./config/apns-key.json`), `APNS_BUNDLE_ID`,
    /// and `APNS_SANDBOX`. Returns `None` if anything required is missing — the
    /// caller in `AppState::build` logs a generic warning and falls back to
    /// `LogPusher`.
    pub fn load_from_env() -> Option<ApnsConfig> {
        let path = std::env::var("APNS_KEY_PATH")
            .unwrap_or_else(|_| "./config/apns-key.json".into());
        let raw = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(_) => return None, // silent: caller will warn at fallback point
        };
        #[derive(serde::Deserialize)]
        struct KeyFile {
            team_id: String,
            key_id: String,
            private_key: String,
        }
        let parsed: KeyFile = match serde_json::from_str(&raw) {
            Ok(k) => k,
            Err(e) => {
                tracing::warn!(
                    target: "relay::push",
                    path = %path,
                    error = %e,
                    "apns key file is not valid JSON; APNs disabled"
                );
                return None;
            }
        };
        let bundle_id = match std::env::var("APNS_BUNDLE_ID") {
            Ok(b) if !b.is_empty() => b,
            _ => {
                tracing::warn!(
                    target: "relay::push",
                    "APNS_BUNDLE_ID not set; APNs disabled"
                );
                return None;
            }
        };
        let sandbox = matches!(
            std::env::var("APNS_SANDBOX").ok().as_deref(),
            Some("1") | Some("true") | Some("TRUE")
        );
        Some(ApnsConfig {
            team_id: parsed.team_id,
            key_id: parsed.key_id,
            private_key_pem: parsed.private_key,
            bundle_id,
            sandbox,
        })
    }
}