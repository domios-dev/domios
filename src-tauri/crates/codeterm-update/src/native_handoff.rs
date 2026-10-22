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
#[serde(deny_unknown_fields)]
pub struct NativeUpdateHandoff {
    pub request: NativeUpdateRequest,
    pub options: NativeUpdateApplyOptions,
    pub result: NativeUpdateApplyResult,
    pub target_version: Option<String>,
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
            request,
            options,
            target_version: None,
        }
    }

    pub fn advance(&mut self, phase: NativeUpdateApplyPhase) -> Result<(), String> {
        self.result.sequence = self
            .result
            .sequence
            .checked_add(1)
            .ok_or("update sequence exhausted")?;
        self.result.phase = phase;
        Ok(())
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
