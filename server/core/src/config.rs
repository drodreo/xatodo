//! Config loading: config.toml
//!
//! Path resolution order: the XATODO_CONFIG env var → config.toml in the
//! working directory.
//!
//! The schema is shared by all three binary forms and stays parse-rigid:
//! the `[server]` and `[users]` sections are required by deserialization
//! even where a form does not consume them.
//! - daemon: listen + database_url + users.entries all consumed
//! - standalone: database_url consumed; listen / users.entries parsed but
//!   not consumed (auth is the shell's token wall; attribution is the
//!   injected local owner)
//! - connect: backend consumed; listen / users.entries parsed but not
//!   consumed (pure forwarder — no local database, no users sync);
//!   database_url unused
//!
//! `database_url` / `backend` are Options because which of them is required
//! is a per-form decision, validated in each bin's main.

use serde::Deserialize;
use xa_todo_entity::user::UserRole;

/// Top-level config
#[derive(Debug, Deserialize)]
pub struct Config {
    /// [server] section (listen consumed by the daemon only)
    pub server: ServerCfg,
    /// Database connection string: sqlite:// or postgres://
    /// (daemon / standalone require it; connect needs no database)
    pub database_url: Option<String>,
    /// apikey authority: synced into the users table at startup (daemon only)
    pub users: UsersCfg,
    /// [backend] section: the daemon declaration for the connect form
    pub backend: Option<BackendCfg>,
}

/// [server] section
#[derive(Debug, Deserialize)]
pub struct ServerCfg {
    /// Listen address, e.g. "0.0.0.0:8600" (daemon only; other forms parse
    /// but do not consume it)
    pub listen: String,
    /// Optional static web root (daemon only): when set, the daemon serves
    /// the directory as a SPA frontend — any GET that misses the API routes
    /// falls back to a file in the directory, and a missing file falls back
    /// to index.html. Absent = no static hosting (API-only behavior,
    /// unchanged). standalone / connect parse but do not consume it.
    pub web_root: Option<String>,
}

/// [backend] section: where the connect form forwards to
#[derive(Debug, Clone, Deserialize)]
pub struct BackendCfg {
    /// Base URL of the daemon, e.g. "http://localhost:8600" (no trailing
    /// slash)
    pub endpoint: String,
    /// Bearer apikey presented to the daemon on every forwarded request
    pub apikey: String,
    /// Whether outbound requests may follow ambient proxy settings:
    /// http_proxy/https_proxy env vars and OS system proxy settings
    /// (macOS SCDynamicStore, Windows registry). Default false — direct
    /// reach: the backend is a user-configured intranet service address.
    /// Set true on networks where the path to the daemon requires a proxy.
    #[serde(default)]
    pub use_system_proxy: bool,
}

/// [users] section
#[derive(Debug, Deserialize)]
pub struct UsersCfg {
    pub entries: Vec<UserEntry>,
}

/// A single [[users.entries]] entry: one user and its apikey
#[derive(Debug, Deserialize)]
pub struct UserEntry {
    pub name: String,
    pub apikey: String,
    /// "user" | "assistant" (reuses the entity enum, serde lowercase form)
    pub role: UserRole,
}

impl Config {
    /// Loads config: the XATODO_CONFIG env var takes precedence; falls back
    /// to config.toml in the working directory
    pub async fn load() -> anyhow::Result<Self> {
        let path = std::env::var("XATODO_CONFIG").unwrap_or_else(|_| "config.toml".to_string());
        let raw = tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| anyhow::anyhow!("failed to read config file ({path}): {e}"))?;
        let cfg: Config = toml::from_str(&raw)
            .map_err(|e| anyhow::anyhow!("failed to parse config file ({path}): {e}"))?;
        Ok(cfg)
    }

    /// Returns the database URL, failing with a form-specific message when
    /// absent (daemon / standalone require it)
    pub fn require_database_url(&self, form: &str) -> anyhow::Result<String> {
        self.database_url.clone().ok_or_else(|| {
            anyhow::anyhow!(
                "config.database_url is missing: required for the {form} form \
                 (top-level database_url in config.toml)"
            )
        })
    }

    /// Returns the backend declaration, failing with a clear message when
    /// absent (connect requires it)
    pub fn require_backend(&self) -> anyhow::Result<&BackendCfg> {
        self.backend.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "config.backend is missing: required for the connect form \
                 ([backend] section with endpoint + apikey in config.toml)"
            )
        })
    }
}
