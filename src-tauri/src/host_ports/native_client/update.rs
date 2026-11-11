use super::*;
use codeterm_client_shell::application_host::{
    DesktopApplicationHost, ServerStop, ServerStopError, ServerStopState,
};
use codeterm_types::native_update::apply::{
    NativeUpdateApplyOptions, NativeUpdateApplyPhase as Phase, NativeUpdateApplyResult,
};
use codeterm_update::{native_handoff::NativeUpdateHandoff, single_flight, UpdateStateHandle};

#[async_trait::async_trait]
impl codeterm_client_shell::application_host::NativeUpdateApply for super::NativeHostClientSlot {
    async fn apply(&self, channel: codeterm_update::Channel) -> Result<serde_json::Value, String> {
        use codeterm_types::native_update::{
            ExpectedServer, NativeUpdateAction, NativeUpdateOperationId,
        };
        use codeterm_types::server_bootstrap::ServerExpectation;
        let client = self
            .0
            .read()
            .clone()
            .ok_or("the native host is unavailable")?;
        let request = NativeUpdateRequest {
            operation_id: NativeUpdateOperationId::new(uuid::Uuid::new_v4())
                .ok_or("native update operation identity is unavailable")?,
            action: NativeUpdateAction::Pair,
            expected_server: ExpectedServer {
                components: client.manifest.clone(),
                identity: client.ready.identity.clone(),
                incarnation: ServerExpectation {
                    process: client.ready.process.clone(),
                    epoch: client.ready.epoch.clone(),
                },
            },
            staged_components: client.manifest.clone(),
            restart_authorized: true,
        };
        let result = client
            .apply_update(
                request,
                NativeUpdateApplyOptions {
                    channel,
                    manifest_url: None,
                    initiating_tab_id: None,
                },
            )
            .await;
        let mut result = match result {
            Ok(result) => result,
            Err(NativeHostRejection::QueueFull) => {
                return Ok(serde_json::json!(
                    codeterm_update::contracts::InstallResponse {
                        status: codeterm_update::contracts::InstallStatus::AlreadyInFlight,
                        message: Some(("An update is already being applied.").to_owned()),
                        ..Default::default()
                    }
                ))
            }
            Err(NativeHostRejection::UpdateAdmission { reason }) => return Err(reason.error()),
            Err(reason) => {
                return Err(format!(
                    "native updater refused before admission: {reason:?}"
                ))
            }
        };
        loop {
            match result.phase {
                Phase::NoUpdate => {
                    return Ok(serde_json::json!(
                        codeterm_update::contracts::InstallResponse {
                            status: codeterm_update::contracts::InstallStatus::UpToDate,
                            ..Default::default()
                        }
                    ))
                }
                Phase::Failed { message } => return Err(message),
                Phase::Relaunching | Phase::Verified { .. } => {
                    return Ok(serde_json::json!(
                        codeterm_update::contracts::InstallResponse {
                            status: codeterm_update::contracts::InstallStatus::Updating,
                            ..Default::default()
                        }
                    ))
                }
                _ => {}
            }
            result = client.observe_update(result.operation_id, result.sequence).await
                .map_err(|reason| format!("native update observation unavailable: {reason:?}; execution was not replayed"))?;
        }
    }
}

pub(crate) struct UpdateOperation {
    record: Mutex<Option<NativeUpdateHandoff>>,
    changed: watch::Sender<u64>,
}

impl Default for UpdateOperation {
    fn default() -> Self {
        Self {
            record: Mutex::new(None),
            changed: watch::channel(0).0,
        }
    }
}

impl UpdateOperation {
    fn recorded_result(
        &self,
        request: &NativeUpdateRequest,
        options: &NativeUpdateApplyOptions,
    ) -> Result<Option<NativeUpdateApplyResult>, NativeHostRejection> {
        self.record
            .lock()
            .as_ref()
            .map(|record| record.recorded_result(request, options))
            .unwrap_or(Ok(None))
    }

    fn admit(
        &self,
        mut record: NativeUpdateHandoff,
        write: impl FnOnce(&NativeUpdateHandoff) -> Result<(), String>,
    ) -> Result<(bool, NativeUpdateApplyResult), NativeHostRejection> {
        let mut guard = self.record.lock();
        if let Some(previous) = guard.as_ref() {
            if let Some(result) = previous.recorded_result(&record.request, &record.options)? {
                return Ok((false, result));
            }
            record.inherit_replay(previous);
        }
        write(&record).map_err(|_| NativeHostRejection::HostRejected)?;
        let result = record.result.clone();
        log::info!(target: "native_updater", "operation={} phase=admitted started_at_ms={}",
            result.operation_id.value(), record.started_at_ms);
        *guard = Some(record);
        self.changed.send_replace(result.sequence);
        Ok((true, result))
    }

    fn mutate(
        &self,
        change: impl FnOnce(&mut NativeUpdateHandoff) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut guard = self.record.lock();
        let mut record = guard
            .clone()
            .ok_or("native update operation is unavailable")?;
        change(&mut record)?;
        if let Err(error) = persist(&record) {
            log::error!(target: "native_updater", "operation={} cause=handoff_persistence",
                record.result.operation_id.value());
            record.advance(Phase::Failed {
                message: "native update outcome could not be persisted; execution was not replayed"
                    .into(),
            })?;
            let sequence = record.result.sequence;
            *guard = Some(record);
            self.changed.send_replace(sequence);
            return Err(error);
        }
        let sequence = record.result.sequence;
        *guard = Some(record);
        self.changed.send_replace(sequence);
        Ok(())
    }
}

fn persist(record: &NativeUpdateHandoff) -> Result<(), String> {
    persist_at(&record.request.expected_server.identity.data, record)
}

fn persist_at(data: &std::path::Path, record: &NativeUpdateHandoff) -> Result<(), String> {
    let path = codeterm_update::native_handoff::path(data);
    codeterm_util::atomic_write::write_atomic(
        &path,
        serde_json::to_vec(record).map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub(super) fn recover(client: &NativeHostClient) -> Result<(), String> {
    if client
        .app
        .state::<Arc<UpdateOperation>>()
        .record
        .lock()
        .is_some()
    {
        return Ok(());
    }
    let Some(mut record) =
        codeterm_update::native_handoff::read_for_recovery(&client.ready.identity.data)
    else {
        return Ok(());
    };
    let installer_refused = std::env::current_exe()
        .ok()
        .and_then(|executable| executable.parent().map(std::path::Path::to_path_buf))
        .is_some_and(|root| {
            codeterm_update::native_handoff::installer_refusal::recover(&root, &mut record)
                .unwrap_or_else(|error| {
                    log::warn!("[updater] could not read installer refusal: {error}");
                    false
                })
        });
    if installer_refused
        || record.recover(
            codeterm_build_info::version(),
            codeterm_build_info::sha(),
            &client.connection,
            &client.ready,
        )
    {
        if let Err(error) = persist_at(&client.ready.identity.data, &record) {
            log::warn!("[updater] could not persist recovered native handoff: {error}");
        }
    }
    if let Phase::Failed { message } = &record.result.phase {
        client
            .app
            .state::<UpdateStateHandle>()
            .lock()
            .set_error(message.clone());
    }
    let operation = client.app.state::<Arc<UpdateOperation>>();
    operation.changed.send_replace(record.result.sequence);
    *operation.record.lock() = Some(record);
    Ok(())
}

impl NativeHostClient {
    pub(super) async fn apply_update(
        self: &Arc<Self>,
        request: NativeUpdateRequest,
        options: NativeUpdateApplyOptions,
    ) -> Result<NativeUpdateApplyResult, NativeHostRejection> {
        if let Err(reason) =
            codeterm_update::native_handoff::validate_apply(&request, &self.ready, &self.manifest)
        {
            return Err(match reason {
                codeterm_types::native_update::NativeUpdateRefusal::ExpectedServerMismatch => NativeHostRejection::UpdateAdmission {
                    reason: codeterm_types::native_update::apply::NativeUpdateAdmissionRefusal::StaleServerIncarnation,
                },
                _ => NativeHostRejection::ProtocolMismatch,
            });
        }
        let operation = self.app.state::<Arc<UpdateOperation>>().inner().clone();
        if let Some(result) = operation.recorded_result(&request, &options)? {
            return Ok(result);
        }
        let lease = single_flight::global()
            .try_acquire()
            .ok_or(NativeHostRejection::QueueFull)?;
        let state = self.app.state::<UpdateStateHandle>().inner().clone();
        let method = state.lock().install_method.clone();
        if !codeterm_update::native_handoff::supports_native_apply(&method) {
            return Err(NativeHostRejection::Unsupported);
        }
        let mut record =
            NativeUpdateHandoff::new(request, options.clone(), self.connection.clone());
        record.owner_client = Some(self.client_process.clone());
        let staging = operation.clone();
        let (is_new, admitted) =
            tokio::task::spawn_blocking(move || staging.admit(record, persist))
                .await
                .map_err(|_| NativeHostRejection::HostRejected)??;
        if !is_new {
            return Ok(admitted);
        }
        {
            let mut state = state.lock();
            state.clear_error();
            state.set_phase(codeterm_update::UpdatePhase::Checking);
        }
        let stop = self.app.state::<ServerStopState>().0.clone();
        let host = DesktopApplicationHost {
            app: self.app.clone(),
            update_state: state.clone(),
            server_stop: Arc::new(UpdateServerStop {
                inner: stop,
                operation: operation.clone(),
            }),
        };
        tauri::async_runtime::spawn(async move {
            let _lease = lease;
            let start = operation.clone();
            let outcome = tauri::async_runtime::spawn(async move {
                tokio::task::spawn_blocking(move || {
                    start.mutate(|record| record.advance(Phase::Downloading))
                })
                .await
                .map_err(|e| e.to_string())??;
                codeterm_update::application_host::ApplicationHost::check_and_apply(
                    &host,
                    codeterm_update::apply::ApplyOptions {
                        channel: options.channel,
                        manifest_url: options.manifest_url,
                        install_method: method,
                        initiating_tab_id: options.initiating_tab_id,
                    },
                )
                .await
            })
            .await
            .unwrap_or_else(|_| Err("native updater task terminated before completion".into()));
            let phase = match outcome {
                Ok(codeterm_update::apply::ApplyResult::NoUpdate) => {
                    let mut state = state.lock();
                    state.clear_error();
                    state.set_phase(codeterm_update::UpdatePhase::Idle);
                    Phase::NoUpdate
                }
                Ok(codeterm_update::apply::ApplyResult::Restarting) => Phase::Relaunching,
                Err(message) => {
                    log::error!(target: "native_updater", "operation_failed cause={}",
                        codeterm_update::native_handoff::failure_code(&message));
                    state.lock().set_error(message.clone());
                    Phase::Failed { message }
                }
            };
            match tokio::task::spawn_blocking(move || {
                operation.mutate(|record| record.advance(phase))
            })
            .await
            {
                Ok(Ok(())) => {}
                Ok(Err(error)) => {
                    log::error!("[updater] native outcome persistence failed: {error}")
                }
                Err(error) => {
                    log::error!("[updater] native outcome persistence worker failed: {error}")
                }
            }
        });
        Ok(admitted)
    }

    pub(super) async fn observe_update(
        &self,
        operation_id: codeterm_types::native_update::NativeUpdateOperationId,
        after_sequence: u64,
    ) -> Result<NativeUpdateApplyResult, NativeHostRejection> {
        let operation = self.app.state::<Arc<UpdateOperation>>().inner().clone();
        let mut changed = operation.changed.subscribe();
        loop {
            {
                let record = operation.record.lock();
                let record = record
                    .as_ref()
                    .filter(|record| record.result.operation_id == operation_id)
                    .ok_or(NativeHostRejection::ProtocolMismatch)?;
                if record.result.sequence > after_sequence {
                    return Ok(record.result.clone());
                }
            }
            changed
                .changed()
                .await
                .map_err(|_| NativeHostRejection::NotAttached)?;
        }
    }
}

struct UpdateServerStop {
    inner: Arc<dyn ServerStop>,
    operation: Arc<UpdateOperation>,
}

#[async_trait::async_trait]
impl ServerStop for UpdateServerStop {
    #[cfg(target_os = "linux")]
    fn debian_quit_target(&self) -> Result<codeterm_update::debian::handoff::QuitTarget, String> {
        self.inner.debian_quit_target()
    }
    fn install_identity(
        &self,
    ) -> Result<codeterm_client_shell::application_host::AttachedServerIdentity, String> {
        self.inner.install_identity()
    }

    async fn prepare_update(&self, version: &str) -> Result<(), String> {
        let operation = self.operation.clone();
        let version = version.to_string();
        tokio::task::spawn_blocking(move || {
            operation.mutate(|record| {
                record.target_version = Some(version);
                record.advance(Phase::Applying)
            })
        })
        .await
        .map_err(|e| e.to_string())?
    }

    async fn stop_server(&self) -> Result<(), ServerStopError> {
        let operation = self.operation.clone();
        tokio::task::spawn_blocking(move || {
            operation.mutate(|record| record.advance(Phase::Relaunching))
        })
        .await
        .map_err(|e| ServerStopError {
            dispatched: false,
            reason: e.to_string(),
        })?
        .map_err(|reason| ServerStopError {
            dispatched: false,
            reason,
        })?;
        self.inner.stop_server().await
    }

    async fn prepare_detached_update(&self, version: &str) -> Result<(), String> {
        self.prepare_update(version).await
    }
}

#[cfg(test)]
mod tests;
