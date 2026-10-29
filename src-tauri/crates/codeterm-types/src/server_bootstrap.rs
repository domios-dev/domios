use serde::{Deserialize, Serialize};
use std::{
    net::SocketAddr,
    path::{Path, PathBuf},
};

/// Protocol 2 Shutdown replies attest successful required cleanup before process exit.
pub const BOOTSTRAP_PROTOCOL: u32 = 2;
pub const API_VERSION: u32 = 3;
pub const SERVER_COMPONENT_SCHEMA: u32 = 2;
pub const CLI_RESOURCE_HINT_FILE: &str = "cli-resources.json";

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CliResourceHint {
    pub resources: PathBuf,
    pub cli_sha256: String,
}
pub const BOOTSTRAP_ATTACH_UNAVAILABLE_EXIT_CODE: i32 = 4;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerComponentManifest {
    pub schema: u32,
    pub server_build: String,
    pub server_executable: PathBuf,
    pub server_sha256: String,
    pub client_identity: ClientIdentity,
    pub api_version: u32,
    pub bootstrap_protocol: u32,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientIdentity {
    None,
    Sha256 {
        sha256: String,
    },
    MacosDesignatedRequirement {
        requirement: String,
        team_id: String,
        identifier: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerIdentity {
    pub data: PathBuf,
    pub config: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProcessProvenance {
    pub pid: u32,
    pub created: String,
    pub executable: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerReady {
    pub protocol: u32,
    pub api_version: u32,
    pub build: String,
    pub instance_id: String,
    pub identity: ServerIdentity,
    pub epoch: String,
    pub process: ProcessProvenance,
    pub api_address: SocketAddr,
    pub socket_address: String,
    pub bootstrap_address: String,
    pub resources: PathBuf,
}

impl ServerReady {
    /// The address a same-machine client dials. A server bound to every interface
    /// (network access enabled) is reached on loopback; a real remote bind is refused.
    pub fn loopback_api_address(&self) -> Option<SocketAddr> {
        let port = self.api_address.port();
        match self.api_address.ip() {
            std::net::IpAddr::V4(ip) if ip.is_unspecified() => {
                Some(SocketAddr::new(std::net::Ipv4Addr::LOCALHOST.into(), port))
            }
            std::net::IpAddr::V6(ip) if ip.is_unspecified() => {
                Some(SocketAddr::new(std::net::Ipv6Addr::LOCALHOST.into(), port))
            }
            ip if ip.is_loopback() => Some(self.api_address),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BootstrapOperation {
    #[default]
    Inspect,
    Shutdown,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerExpectation {
    pub process: ProcessProvenance,
    pub epoch: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachRequest {
    pub protocol: u32,
    pub api_version: u32,
    pub build: String,
    pub identity: ServerIdentity,
    pub challenge: String,
    #[serde(default)]
    pub operation: BootstrapOperation,
    pub expected: Option<ServerExpectation>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AttachReply {
    pub challenge: String,
    pub ready: ServerReady,
    pub api_token: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum InstanceSelector {
    Production,
    Development {
        instance: Option<String>,
        port: u16,
    },
    IsolatedVerification {
        data_dir: PathBuf,
        config_dir: PathBuf,
        bind: SocketAddr,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchOptions {
    pub instance: InstanceSelector,
    pub resources: PathBuf,
    pub probe_sink: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_identity_alias: Option<ServerIdentity>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub development_owner: Option<ProcessProvenance>,
}

impl LaunchOptions {
    pub fn validate(&self, development_artifact: bool) -> std::io::Result<()> {
        if self.development_owner.is_some()
            && (!development_artifact
                || !matches!(&self.instance, InstanceSelector::Development { .. }))
        {
            return Err(std::io::Error::other(
                "Launch ownership requires a development artifact",
            ));
        }
        if self.probe_identity_alias.is_some()
            && (!matches!(&self.instance, InstanceSelector::Development { instance: Some(instance), .. } if instance.starts_with("split-oracle-"))
                || self.probe_sink.is_none())
        {
            return Err(std::io::Error::other(
                "Identity aliases require an isolated process probe",
            ));
        }
        match &self.instance {
            InstanceSelector::Production if !development_artifact => Ok(()),
            InstanceSelector::Development { instance, port } if development_artifact => {
                if *port == 0
                    || *port == 7685
                    || instance.as_ref().is_some_and(|s| {
                        s.is_empty()
                            || s.len() > 64
                            || !s
                                .bytes()
                                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
                    })
                {
                    Err(std::io::Error::other(
                        "Invalid isolated development selector",
                    ))
                } else {
                    Ok(())
                }
            }
            InstanceSelector::IsolatedVerification {
                data_dir,
                config_dir,
                bind,
            } => validate_isolated_verification(
                data_dir,
                config_dir,
                *bind,
                &dirs::home_dir()
                    .ok_or_else(|| std::io::Error::other("Cannot determine the user data root"))?,
            ),
            _ => Err(std::io::Error::other(
                "Instance selector does not match server artifact",
            )),
        }
    }
}

fn validate_isolated_verification(
    data_dir_path: &Path,
    config_dir_path: &Path,
    bind: SocketAddr,
    home: &Path,
) -> std::io::Result<()> {
    if !bind.ip().is_loopback() || matches!(bind.port(), 0 | 7685 | 7686) {
        return Err(std::io::Error::other(
            "Isolated verification requires a private loopback port",
        ));
    }
    let data_dir = canonical_isolated_directory(data_dir_path, "data")?;
    let config_dir = canonical_isolated_directory(config_dir_path, "config")?;
    if data_dir == config_dir {
        return Err(std::io::Error::other(
            "Isolated verification data and config directories must differ",
        ));
    }
    validate_isolated_config_file(&config_dir)?;
    let home = home.canonicalize()?;
    if is_user_codeterm_directory(data_dir_path, &home)
        || is_user_codeterm_directory(config_dir_path, &home)
        || is_user_codeterm_directory(&data_dir, &home)
        || is_user_codeterm_directory(&config_dir, &home)
    {
        return Err(std::io::Error::other(
            "Isolated verification refuses user Domios directories",
        ));
    }
    Ok(())
}

fn validate_isolated_config_file(config_dir: &Path) -> std::io::Result<()> {
    let config_file = config_dir.join("config.toml");
    match std::fs::symlink_metadata(&config_file) {
        Ok(_) => {
            let resolved = config_file.canonicalize()?;
            if resolved.parent() != Some(config_dir) {
                return Err(std::io::Error::other(
                    "Isolated verification config file must stay in its config directory",
                ));
            }
            Ok(())
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

fn canonical_isolated_directory(path: &Path, label: &str) -> std::io::Result<PathBuf> {
    if !path.is_absolute() {
        return Err(std::io::Error::other(format!(
            "Isolated verification {label} directory must be absolute"
        )));
    }
    let canonical = path.canonicalize()?;
    if !canonical.is_dir() {
        return Err(std::io::Error::other(format!(
            "Isolated verification {label} path must be a directory"
        )));
    }
    Ok(canonical)
}

fn is_user_codeterm_directory(path: &Path, home: &Path) -> bool {
    let Ok(relative) = path.strip_prefix(home) else {
        return false;
    };
    relative.components().any(|component| {
        let name = component.as_os_str().to_string_lossy();
        if cfg!(windows) {
            let name = name.to_ascii_lowercase();
            name.starts_with(".domios") || name.starts_with(".codeterm")
        } else {
            name.starts_with(".domios") || name.starts_with(".codeterm")
        }
    })
}

pub const DIAGNOSTIC_LINE_CHARS: usize = 480;

/// A bootstrap child that refuses before its logger exists leaves its cause only on stderr.
pub fn last_diagnostic_line(bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(bytes);
    let line = text
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())?;
    Some(line.chars().take(DIAGNOSTIC_LINE_CHARS).collect())
}

#[cfg(test)]
mod tests;
