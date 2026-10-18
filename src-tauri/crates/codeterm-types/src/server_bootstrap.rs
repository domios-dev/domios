use serde::{Deserialize, Serialize};
use std::{net::SocketAddr, path::PathBuf};

pub const BOOTSTRAP_PROTOCOL: u32 = 1;
pub const API_VERSION: u32 = 3;
pub const SERVER_COMPONENT_SCHEMA: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ServerComponentManifest {
    pub schema: u32,
    pub server_build: String,
    pub server_executable: PathBuf,
    pub server_sha256: String,
    pub client_sha256: String,
    pub api_version: u32,
    pub bootstrap_protocol: u32,
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
    Development { instance: Option<String>, port: u16 },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct LaunchOptions {
    pub instance: InstanceSelector,
    pub resources: PathBuf,
    pub probe_sink: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub probe_identity_alias: Option<ServerIdentity>,
}

impl LaunchOptions {
    pub fn validate(&self, development_artifact: bool) -> std::io::Result<()> {
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
            _ => Err(std::io::Error::other(
                "Instance selector does not match server artifact",
            )),
        }
    }
}
