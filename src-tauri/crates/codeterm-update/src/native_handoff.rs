pub mod replay;
use codeterm_types::{
    native_host::NativeHostConnection,
    native_update::{
        apply::{NativeUpdateApplyOptions, NativeUpdateApplyPhase, NativeUpdateApplyResult},
        NativeUpdateRequest,
    },
    server_bootstrap::ServerReady,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub fn supports_native_apply(method: &crate::InstallMethod) -> bool {
    matches!(
        method,
        crate::InstallMethod::Dmg
            | crate::InstallMethod::Nsis
            | crate::InstallMethod::Msi
            | crate::InstallMethod::AppImage
            | crate::InstallMethod::Deb
    )
}

pub fn validate_apply(
    request: &NativeUpdateRequest,
    ready: &ServerReady,
    manifest: &codeterm_types::server_bootstrap::ServerComponentManifest,
) -> Result<(), codeterm_types::native_update::NativeUpdateRefusal> {
    use codeterm_types::native_update::{NativeUpdateAction, NativeUpdateRefusal};
    request.validate()?;
    if request.action != NativeUpdateAction::Pair {
        return Err(NativeUpdateRefusal::DurableHandoffUnavailable);
    }
    if !request.matches_current_server(ready, manifest) {
        return Err(NativeUpdateRefusal::ExpectedServerMismatch);
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct NativeUpdateHandoff {
    pub request: NativeUpdateRequest,
    pub options: NativeUpdateApplyOptions,
    pub result: NativeUpdateApplyResult,
    pub target_version: Option<String>,
    #[serde(default)]
    pub replay: replay::ReplayLedger,
    #[serde(default)]
    pub started_at_ms: u64,
    #[serde(default)]
    pub updated_at_ms: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner_client: Option<codeterm_types::server_bootstrap::ProcessProvenance>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub terminal_cause: Option<String>,
}

pub fn path(data: &Path) -> PathBuf {
    data.join("native-update-handoff.json")
}

pub fn read(data: &Path) -> Result<Option<NativeUpdateHandoff>, String> {
    match std::fs::read(path(data)) {
        Ok(bytes) => serde_json::from_slice(&bytes)
            .map(Some)
            .map_err(|e| e.to_string()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

pub fn read_optional(data: &Path) -> Option<NativeUpdateHandoff> {
    match read(data) {
        Ok(record) => record,
        Err(error) => {
            log::warn!("[updater] ignoring unreadable native handoff: {error}");
            None
        }
    }
}

pub fn read_for_recovery(data: &Path) -> Option<NativeUpdateHandoff> {
    match read(data) {
        Ok(record) => record,
        Err(error) => {
            let quarantine = data.join(format!(
                "native-update-handoff.rejected-{}.json",
                uuid::Uuid::new_v4()
            ));
            log::warn!("[updater] quarantining invalid native handoff: {error}");
            if let Err(error) = std::fs::rename(path(data), quarantine) {
                log::warn!("[updater] could not quarantine native handoff: {error}");
            }
            None
        }
    }
}

impl NativeUpdateHandoff {
    pub fn new(
        request: NativeUpdateRequest,
        options: NativeUpdateApplyOptions,
        connection: NativeHostConnection,
    ) -> Self {
        Self {
            result: NativeUpdateApplyResult {
                operation_id: request.operation_id,
                connection,
                sequence: 1,
                phase: NativeUpdateApplyPhase::Admitted,
            },
            replay: replay::ReplayLedger::new(request.operation_id),
            request,
            options,
            target_version: None,
            started_at_ms: now_ms(),
            updated_at_ms: now_ms(),
            owner_client: None,
            started_version: Some(codeterm_build_info::version().into()),
            terminal_cause: None,
        }
    }

    pub fn recorded_result(
        &self,
        request: &NativeUpdateRequest,
        options: &NativeUpdateApplyOptions,
    ) -> Result<Option<NativeUpdateApplyResult>, codeterm_types::native_host::NativeHostRejection>
    {
        use codeterm_types::{
            native_host::NativeHostRejection,
            native_update::apply::NativeUpdateAdmissionRefusal as Refusal,
        };
        let same_incarnation = replay::same_incarnation(&self.request, request);
        let previous = if same_incarnation && self.request.operation_id == request.operation_id {
            Some((&self.request, &self.options, &self.result))
        } else {
            self.replay
                .recent
                .iter()
                .find(|prior| {
                    prior.request.operation_id == request.operation_id
                        && replay::same_incarnation(&prior.request, request)
                })
                .map(|prior| (&prior.request, &prior.options, &prior.result))
        };
        if let Some((prior_request, prior_options, result)) = previous {
            return if prior_request == request && prior_options == options {
                Ok(Some(result.clone()))
            } else {
                Err(NativeHostRejection::UpdateAdmission {
                    reason: Refusal::OperationConflict,
                })
            };
        }
        if same_incarnation && self.replay.contains(request.operation_id) {
            return Err(NativeHostRejection::UpdateAdmission {
                reason: Refusal::RetiredOperation,
            });
        }
        if self.blocks_new_attempt() {
            return Err(NativeHostRejection::QueueFull);
        }
        if same_incarnation && self.replay.at_capacity(self.request.operation_id) {
            return Err(NativeHostRejection::UpdateAdmission {
                reason: Refusal::IncarnationLimit,
            });
        }
        Ok(None)
    }

    pub fn inherit_replay(&mut self, previous: &Self) {
        self.replay = replay::ReplayLedger::after(previous, &self.request);
    }

    pub fn blocks_new_attempt(&self) -> bool {
        matches!(
            self.result.phase,
            NativeUpdateApplyPhase::Admitted
                | NativeUpdateApplyPhase::Downloading
                | NativeUpdateApplyPhase::Applying
                | NativeUpdateApplyPhase::Relaunching
        )
    }

    pub fn advance(&mut self, phase: NativeUpdateApplyPhase) -> Result<(), String> {
        self.result.sequence = self
            .result
            .sequence
            .checked_add(1)
            .ok_or("update sequence exhausted")?;
        self.updated_at_ms = now_ms();
        self.terminal_cause = match &phase {
            NativeUpdateApplyPhase::Failed { message } => Some(message.clone()),
            NativeUpdateApplyPhase::NoUpdate => Some("no applicable update".into()),
            NativeUpdateApplyPhase::Verified { .. } => Some("replacement pair verified".into()),
            _ => None,
        };
        log::info!(target: "native_updater", "operation={} sequence={} phase={}",
            self.result.operation_id.value(), self.result.sequence, phase_name(&phase));
        self.result.phase = phase;
        Ok(())
    }

    pub fn recover(
        &mut self,
        version: &str,
        client_build: &str,
        connection: &NativeHostConnection,
        server: &ServerReady,
    ) -> bool {
        if self.request.expected_server.identity == server.identity
            && server.build == client_build
            && connection.identity == server.identity
            && connection.server_epoch == server.epoch
            && connection.instance_id == server.instance_id
            && connection.generation > 0
            && (!matches!(self.result.phase, NativeUpdateApplyPhase::Relaunching)
                || (server.epoch != self.request.expected_server.incarnation.epoch
                    && server.process != self.request.expected_server.incarnation.process))
            && self.superseded_by(version)
        {
            log::info!(target: "native_updater", "operation={} recovery=superseded current_version={} execution_replayed=false",
                self.result.operation_id.value(), version);
            return self.advance(NativeUpdateApplyPhase::NoUpdate).is_ok();
        }
        if matches!(
            self.result.phase,
            NativeUpdateApplyPhase::Failed { .. }
                | NativeUpdateApplyPhase::Verified { .. }
                | NativeUpdateApplyPhase::NoUpdate
        ) {
            return false;
        }
        let outcome = if self.request.expected_server.identity != server.identity {
            Err("native update handoff belongs to another instance".into())
        } else {
            self.reconcile(version, client_build, connection, server)
        };
        let failure = match outcome {
            Ok(true) => return true,
            Ok(false)
                if matches!(
                    self.result.phase,
                    NativeUpdateApplyPhase::Admitted
                        | NativeUpdateApplyPhase::Downloading
                        | NativeUpdateApplyPhase::Applying
                ) =>
            {
                format!(
                    "native update owner stopped during {}; server {}; execution was not replayed",
                    phase_name(&self.result.phase),
                    if self.request.expected_server.incarnation.epoch == server.epoch {
                        "unchanged"
                    } else {
                        "replaced"
                    }
                )
            }
            Ok(false) => return false,
            Err(error) => error,
        };
        log::warn!(target: "native_updater", "operation={} recovery=interrupted prior_phase={} prior_client_pid={:?} server_changed={} execution_replayed=false",
            self.result.operation_id.value(), phase_name(&self.result.phase),
            self.owner_client.as_ref().map(|owner| owner.pid),
            self.request.expected_server.incarnation.epoch != server.epoch);
        self.advance(NativeUpdateApplyPhase::Failed { message: failure })
            .is_ok()
    }

    fn superseded_by(&self, version: &str) -> bool {
        if matches!(
            self.result.phase,
            NativeUpdateApplyPhase::Verified { .. } | NativeUpdateApplyPhase::NoUpdate
        ) {
            return false;
        }
        let Ok(current) = semver::Version::parse(version) else {
            return false;
        };
        if let Some(target) = self
            .target_version
            .as_ref()
            .and_then(|v| semver::Version::parse(v).ok())
        {
            return if matches!(self.result.phase, NativeUpdateApplyPhase::Relaunching) {
                current > target
            } else {
                current >= target
            };
        }
        if matches!(self.result.phase, NativeUpdateApplyPhase::Relaunching) {
            return false;
        }
        self.started_version
            .as_ref()
            .and_then(|v| semver::Version::parse(v).ok())
            .is_some_and(|started| current > started)
    }

    pub fn reconcile(
        &mut self,
        version: &str,
        client_build: &str,
        connection: &NativeHostConnection,
        server: &ServerReady,
    ) -> Result<bool, String> {
        if !matches!(self.result.phase, NativeUpdateApplyPhase::Relaunching) {
            return Ok(false);
        }
        if connection.identity != self.request.expected_server.identity
            || connection.identity != server.identity
            || connection.server_epoch != server.epoch
            || connection.instance_id != server.instance_id
            || connection.generation == 0
            || server.epoch == self.request.expected_server.incarnation.epoch
            || server.process == self.request.expected_server.incarnation.process
        {
            return Err("update replacement does not own the expected instance".into());
        }
        let phase = if self.target_version.as_deref() == Some(version)
            && server.build == client_build
        {
            NativeUpdateApplyPhase::Verified {
                version: version.into(),
                server: Box::new(server.clone()),
            }
        } else {
            NativeUpdateApplyPhase::Failed {
                message: "the relaunched pair does not match the verified installer version and client/server build".into(),
            }
        };
        self.advance(phase)?;
        self.result.connection = connection.clone();
        Ok(true)
    }
}

#[cfg(test)]
mod tests;

pub fn phase_name(phase: &NativeUpdateApplyPhase) -> &'static str {
    match phase {
        NativeUpdateApplyPhase::Admitted => "admitted",
        NativeUpdateApplyPhase::Downloading => "downloading",
        NativeUpdateApplyPhase::Applying => "applying",
        NativeUpdateApplyPhase::Relaunching => "relaunching",
        NativeUpdateApplyPhase::NoUpdate => "no_update",
        NativeUpdateApplyPhase::Failed { .. } => "failed",
        NativeUpdateApplyPhase::Verified { .. } => "verified",
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or_default()
}

pub fn failure_code(message: &str) -> &'static str {
    if message.contains("install-file holders") {
        "install_file_holders"
    } else if message.contains("download/verify") {
        "download_or_signature"
    } else if message.contains("manifest") || message.contains("fetch release") {
        "manifest_fetch"
    } else if message.contains("server") && message.contains("stop") {
        "server_stop"
    } else if message.contains("installer launch") {
        "installer_launch"
    } else if message.contains("task terminated") {
        "task_terminated"
    } else {
        "operation_failed"
    }
}
