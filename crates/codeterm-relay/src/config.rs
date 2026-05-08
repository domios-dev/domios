//! Configuration loading and types.

use serde::{Deserialize, Serialize};
use std::path::Path;
use anyhow::{Context, Result};
use codeterm_mesh_client::{generate_peer_keypair, public_key_from_private};

/// Main configuration structure.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    #[serde(default)]
    pub relay: RelayConfig,

    #[serde(default)]
    pub mesh: MeshConfig,

    #[serde(default)]
    pub logging: LoggingConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            relay: RelayConfig::default(),
            mesh: MeshConfig::default(),
            logging: LoggingConfig::default(),
        }
    }
}

/// Relay-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// Instance name (shown in mesh)
    #[serde(default = "hostname_or_default")]
    pub name: String,

    /// API bind address
    pub bind: String,

    /// HTTP port
    pub port: u16,

    /// Bearer token for API authentication
    #[serde(default)]
    pub bearer_token: Option<String>,
}

fn hostname_or_default() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "relay".into())
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            name: hostname_or_default(),
            bind: "0.0.0.0:7690".into(),
            port: 7690,
            bearer_token: None,
        }
    }
}

/// Mesh-specific configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Mesh API port (for full peers to connect)
    pub api_port: u16,

    /// Ed25519 private key (base64) - generated if not present
    #[serde(default)]
    pub private_key: Option<String>,
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            api_port: 7691,
            private_key: None,
        }
    }
}

/// Logging configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "info".into()
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
        }
    }
}

impl Config {
    /// Load config from file or create default.
    pub fn load_or_create(path: &Path, args: &crate::Args) -> Result<Self> {
        let mut config = if path.exists() {
            let contents = std::fs::read_to_string(path)
                .context("Failed to read config file")?;
            toml::from_str(&contents)
                .context("Failed to parse config file")?
        } else {
            Config::default()
        };

        // Override with CLI args if provided
        if let Some(token) = &args.bearer_token {
            config.relay.bearer_token = Some(token.clone());
        }

        // Save default config if it didn't exist
        if !path.exists() {
            let contents = toml::to_string_pretty(&config)
                .context("Failed to serialize config")?;
            std::fs::write(path, &contents)
                .context("Failed to write config file")?;
        }

        Ok(config)
    }
}

impl codeterm_mesh_client::types::MeshIdentity {
    /// Load identity from file or create new.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        let mut identity = if path.exists() {
            let contents = std::fs::read_to_string(path)
                .context("Failed to read identity file")?;
            serde_json::from_str(&contents)
                .context("Failed to parse identity file")?
        } else {
            // Create new identity with generated keypair
            let (priv_key, pub_key) = generate_peer_keypair();
            Self {
                instance_id: uuid::Uuid::new_v4().to_string(),
                private_key: Some(priv_key),
                public_key: Some(pub_key),
            }
        };

        // Ensure we have a keypair (backwards compatibility)
        if identity.private_key.is_none() || identity.public_key.is_none() {
            let (priv_key, pub_key) = generate_peer_keypair();
            identity.private_key = Some(priv_key);
            identity.public_key = Some(pub_key);
        }

        // Save if new or missing keys
        if !path.exists() || identity.public_key.is_none() {
            identity.save(path)?;
        }

        Ok(identity)
    }

    /// Save identity to file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let contents = serde_json::to_string_pretty(&self)
            .context("Failed to serialize identity")?;
        std::fs::write(path, &contents)
            .context("Failed to write identity file")?;
        Ok(())
    }

    /// Get the instance name (generated from hostname).
    pub fn instance_name(&self) -> String {
        // Try stored name first, fall back to hostname
        hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "relay".into())
    }
}
