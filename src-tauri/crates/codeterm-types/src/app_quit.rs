use crate::{
    native_host::NativeHostConnection,
    server_bootstrap::{
        ProcessProvenance, ServerIdentity, ServerReady, API_VERSION, BOOTSTRAP_PROTOCOL,
    },
};
use serde::{Deserialize, Serialize};
use std::time::Duration;

pub const QUIT_FAILURE_DEADLINE: Duration = Duration::from_secs(30);
pub const QUIT_RECEIPT_SCHEMA: u32 = 1;
pub const MAX_QUIT_SNAPSHOT_BYTES: usize = 8 * 1024;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuitClient {
    pub connection: NativeHostConnection,
    pub process: ProcessProvenance,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuitSnapshot {
    pub ready: ServerReady,
    pub client: Option<QuitClient>,
}

impl PartialEq for QuitSnapshot {
    fn eq(&self, other: &Self) -> bool {
        let (left, right) = (&self.ready, &other.ready);
        left.protocol == right.protocol
            && left.api_version == right.api_version
            && left.build == right.build
            && left.instance_id == right.instance_id
            && left.identity == right.identity
            && left.epoch == right.epoch
            && left.process == right.process
            && left.resources == right.resources
            && left.api_address == right.api_address
            && left.socket_address == right.socket_address
            && left.bootstrap_address == right.bootstrap_address
            && self.client == other.client
    }
}
impl QuitSnapshot {
    pub fn matches(&self, other: &Self) -> bool {
        self.valid() && other.valid() && self == other
    }
    pub fn valid(&self) -> bool {
        let ready = &self.ready;
        serde_json::to_vec(self).is_ok_and(|bytes| bytes.len() <= MAX_QUIT_SNAPSHOT_BYTES)
            && ready.protocol == BOOTSTRAP_PROTOCOL
            && ready.api_version == API_VERSION
            && !ready.build.is_empty()
            && !ready.epoch.is_empty()
            && !ready.instance_id.is_empty()
            && valid_process(&ready.process)
            && ready.identity.data.is_absolute()
            && ready.identity.config.is_absolute()
            && self.client.as_ref().is_none_or(|client| {
                valid_process(&client.process)
                    && client.process.pid != ready.process.pid
                    && client.connection.generation != 0
                    && client.connection.server_epoch == ready.epoch
                    && client.connection.instance_id == ready.instance_id
                    && client.connection.identity == ready.identity
            })
    }
}

fn valid_process(process: &ProcessProvenance) -> bool {
    process.pid != 0 && !process.created.is_empty() && process.executable.is_absolute()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuitRefusal {
    Busy,
    UnsavedWork,
    NoInstance,
    MismatchedBuild,
    Unavailable,
    IdentityMismatch,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuitUncertainty {
    Deadline,
    Disconnected,
    InvalidCompletion,
    ExitUnobserved,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuitOperation {
    Shutdown,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuitCleanup {
    Completed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuitCompletion {
    pub operation: QuitOperation,
    pub cleanup: QuitCleanup,
}
impl Default for QuitCompletion {
    fn default() -> Self {
        Self {
            operation: QuitOperation::Shutdown,
            cleanup: QuitCleanup::Completed,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuitApiReply {
    Completed {
        expected: Box<QuitSnapshot>,
        shutdown_completion: QuitCompletion,
    },
    Refused {
        reason: QuitRefusal,
    },
    Uncertain {
        reason: QuitUncertainty,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum QuitExit {
    Observed,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuitExitedProcess {
    pub process: ProcessProvenance,
    pub exit: QuitExit,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct QuitReceipt {
    pub schema: u32,
    pub bootstrap_protocol: u32,
    pub api_version: u32,
    pub server_build: String,
    pub instance_id: String,
    pub identity: ServerIdentity,
    pub server_epoch: String,
    pub shutdown_completion: QuitCompletion,
    pub server: QuitExitedProcess,
    pub client: Option<QuitExitedProcess>,
}

impl QuitReceipt {
    /// Call only after protected completion and retained kernel witnesses prove both exits.
    pub fn after_observed_exits(
        snapshot: QuitSnapshot,
        shutdown_completion: QuitCompletion,
    ) -> Self {
        let ready = snapshot.ready;
        Self {
            schema: QUIT_RECEIPT_SCHEMA,
            bootstrap_protocol: ready.protocol,
            api_version: ready.api_version,
            server_build: ready.build,
            instance_id: ready.instance_id,
            identity: ready.identity,
            server_epoch: ready.epoch,
            shutdown_completion,
            server: QuitExitedProcess {
                process: ready.process,
                exit: QuitExit::Observed,
            },
            client: snapshot.client.map(|client| QuitExitedProcess {
                process: client.process,
                exit: QuitExit::Observed,
            }),
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "outcome", rename_all = "snake_case", deny_unknown_fields)]
pub enum QuitOutput {
    Confirmed { receipt: Box<QuitReceipt> },
    Refused { reason: QuitRefusal },
    Uncertain { reason: QuitUncertainty },
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn output_never_confuses_admission_or_uncertainty_with_completion() {
        let output = QuitOutput::Uncertain {
            reason: QuitUncertainty::ExitUnobserved,
        };
        let value = serde_json::to_value(output).unwrap();
        assert_eq!(
            value,
            serde_json::json!({"outcome":"uncertain","reason":"exit_unobserved"})
        );
        assert!(
            serde_json::from_value::<QuitOutput>(serde_json::json!({"outcome":"confirmed"}))
                .is_err()
        );
        assert!(serde_json::from_value::<QuitCompletion>(
            serde_json::json!({"operation":"shutdown","cleanup":"admitted"})
        )
        .is_err());
    }
    fn snapshot() -> QuitSnapshot {
        use crate::server_bootstrap::{
            ProcessProvenance, ServerIdentity, ServerReady, API_VERSION, BOOTSTRAP_PROTOCOL,
        };
        let root = std::path::PathBuf::from(if cfg!(windows) {
            "C:/quit-fixture"
        } else {
            "/quit-fixture"
        });
        QuitSnapshot {
            ready: ServerReady {
                protocol: BOOTSTRAP_PROTOCOL,
                api_version: API_VERSION,
                build: "expected-server-build".into(),
                instance_id: "instance-a".into(),
                identity: ServerIdentity {
                    data: root.join("data"),
                    config: root.join("config.toml"),
                },
                epoch: "epoch-a".into(),
                process: ProcessProvenance {
                    pid: 42,
                    created: "start-a".into(),
                    executable: root.join("server"),
                },
                api_address: "127.0.0.1:18886".parse().unwrap(),
                socket_address: "socket".into(),
                bootstrap_address: "bootstrap".into(),
                resources: root.join("resources"),
            },
            client: None,
        }
    }

    #[test]
    fn snapshot_fences_protocol_epoch_pid_reuse_and_client_generation() {
        let expected = snapshot();
        assert!(expected.valid());
        for field in ["protocol", "epoch", "creation", "config"] {
            let mut other = expected.clone();
            match field {
                "protocol" => other.ready.protocol = BOOTSTRAP_PROTOCOL - 1,
                "epoch" => other.ready.epoch = "replaced".into(),
                "creation" => other.ready.process.created = "reused".into(),
                _ => other.ready.identity.config = other.ready.identity.data.join("foreign.toml"),
            }
            assert!(!expected.matches(&other));
        }
        let mut expected = expected;
        expected.client = Some(QuitClient {
            connection: NativeHostConnection {
                instance_id: expected.ready.instance_id.clone(),
                identity: expected.ready.identity.clone(),
                server_epoch: expected.ready.epoch.clone(),
                generation: 7,
            },
            process: ProcessProvenance {
                pid: 43,
                created: "client-created".into(),
                executable: expected.ready.resources.join("client"),
            },
        });
        assert!(expected.valid());
        let mut other = expected.clone();
        other.client.as_mut().unwrap().connection.generation += 1;
        assert!(!expected.matches(&other));
        other = expected.clone();
        other.client.as_mut().unwrap().process.created = "reused-client".into();
        assert!(!expected.matches(&other));
        other = expected.clone();
        other.client = None;
        assert!(!expected.matches(&other));
    }
}
