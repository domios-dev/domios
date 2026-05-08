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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayConfig {
    /// Instance name (shown in mesh)
    #[serde(default = "default_relay_name")]
    pub name: String,

    /// API bind address
    #[serde(default = "default_relay_bind")]
    pub bind: String,

    /// HTTP port
    #[serde(default = "default_port")]
    pub port: u16,

    /// Bearer token for API authentication
    #[serde(default)]
    pub bearer_token: Option<String>,
}

fn default_relay_name() -> String {
    hostname::get()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_else(|_| "relay".to_string())
}

fn default_relay_bind() -> String {
    "0.0.0.0:7690".to_string()
}

fn default_port() -> u16 {
    7690
}

impl Default for RelayConfig {
    fn default() -> Self {
        Self {
            name: default_relay_name(),
            bind: default_relay_bind(),
            port: default_port(),
            bearer_token: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshConfig {
    /// Mesh API port (for full peers to connect)
    #[serde(default = "default_mesh_port")]
    pub api_port: u16,

    /// Ed25519 private key (base64) - generated if not present
    #[serde(default)]
    pub private_key: Option<String>,
}

fn default_mesh_port() -> u16 {
    7691
}

impl Default for MeshConfig {
    fn default() -> Self {
        Self {
            api_port: default_mesh_port(),
            private_key: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level: trace, debug, info, warn, error
    #[serde(default = "default_log_level")]
    pub level: String,
}

fn default_log_level() -> String {
    "info".to_string()
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
        if path.exists() {
            let contents = std::fs::read_to_string(path)
                .context("Failed to read config file")?;
            let mut config: Config = toml::from_str(&contents)
                .context("Failed to parse config file")?;

            // Override with CLI args if provided
            if let Some(token) = &args.bearer_token {
                config.relay.bearer_token = Some(token.clone());
            }

            Ok(config)
        } else {
            // Create default config
            let mut config = Config::default();

            // Override with CLI args if provided
            if let Some(token) = &args.bearer_token {
                config.relay.bearer_token = Some(token.clone());
            }

            // Save default config
            let contents = toml::to_string_pretty(&config)
                .context("Failed to serialize config")?;
            std::fs::write(path, &contents)
                .context("Failed to write config file")?;

            Ok(config)
        }
    }
}

/// Identity for mesh participants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayIdentity {
    pub instance_id: String,
    pub instance_name: String,
    /// Ed25519 private key (base64)
    pub private_key: Option<String>,
    /// Ed25519 public key (base64)
    pub public_key: Option<String>,
}

impl RelayIdentity {
    /// Load identity from file or create new.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)
                .context("Failed to read identity file")?;
            let mut identity: RelayIdentity = serde_json::from_str(&contents)
                .context("Failed to parse identity file")?;

            // Ensure we have a keypair
            if identity.private_key.is_none() || identity.public_key.is_none() {
                let (priv_key, pub_key) = generate_peer_keypair();
                identity.private_key = Some(priv_key);
                identity.public_key = Some(pub_key);
                identity.save(path)?;
            }

            Ok(identity)
        } else {
            // Create new identity
            let (priv_key, pub_key) = generate_peer_keypair();
            let instance_id = uuid::Uuid::new_v4().to_string();
            let instance_name = hostname::get()
                .map(|h| h.to_string_lossy().to_string())
                .unwrap_or_else(|_| "relay".to_string());

            let identity = Self {
                instance_id,
                instance_name,
                private_key: Some(priv_key),
                public_key: Some(pub_key),
            };

            identity.save(path)?;

            Ok(identity)
        }
    }

    /// Save identity to file.
    pub fn save(&self, path: &Path) -> Result<()> {
        let contents = serde_json::to_string_pretty(&self)
            .context("Failed to serialize identity")?;
        std::fs::write(path, &contents)
            .context("Failed to write identity file")?;
        Ok(())
    }

    /// Get the instance name.
    pub fn instance_name(&self) -> &str {
        &self.instance_name
    }
}

impl codeterm_mesh_client::types::MeshIdentity {
    /// Load or create identity.
    pub fn load_or_create(path: &Path) -> Result<Self> {
        if path.exists() {
            let contents = std::fs::read_to_string(path)
                .context("Failed to read identity file")?;
            let mut identity: Self = serde_json::from_str(&contents)
                .context("Failed to parse identity file")?;

            // Ensure we have a keypair
            if identity.private_key.is_none() || identity.public_key.is_none() {
                let (priv_key, pub_key) = generate_peer_keypair();
                identity.private_key = Some(priv_key);
                identity.public_key = Some(pub_key);
                identity.save(path)?;
            }

            Ok(identity)
        } else {
            // Create new identity
            let (priv_key, pub_key) = generate_peer_keypair();
            let instance_id = uuid::Uuid::new_v4().to_string();

            let identity = Self {
                instance_id,
                private_key: Some(priv_key),
                public_key: Some(pub_key),
            };

            identity.save(path)?;

            Ok(identity)
        }
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
        hostname::get()
            .map(|h| h.to_string_lossy().to_string())
            .unwrap_or_else(|_| "relay".to_string())
    }
}