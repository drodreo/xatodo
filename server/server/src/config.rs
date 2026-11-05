//! Config loading: config.toml
//!
//! Path resolution order: the XATODO_CONFIG env var → config.toml in the
//! working directory.

use serde::Deserialize;
use xa_todo_entity::user::UserRole;

/// Top-level config
#[derive(Debug, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    /// apikey authority: synced into the users table at startup
    pub users: UsersConfig,
}

/// [server] section
#[derive(Debug, Deserialize)]
pub struct ServerConfig {
    /// Listen address, e.g. "0.0.0.0:8600"
    pub listen: String,
    /// Database connection string: sqlite:// or postgres://
    pub database_url: String,
}

/// [users] section
#[derive(Debug, Deserialize)]
pub struct UsersConfig {
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
}
