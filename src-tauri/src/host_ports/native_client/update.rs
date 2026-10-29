use super::*;
use codeterm_client_shell::application_host::{
    DesktopApplicationHost, ServerStop, ServerStopError, ServerStopState,
};
use codeterm_types::native_update::apply::{
    NativeUpdateApplyOptions, NativeUpdateApplyPhase as Phase, NativeUpdateApplyResult,
};
use codeterm_update::{native_handoff::NativeUpdateHandoff, single_flight, UpdateStateHandle};

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
    fn mutate(
        &self,
        change: impl FnOnce(&mut NativeUpdateHandoff) -> Result<(), String>,
    ) -> Result<(), String> {
        let mut guard = self.record.lock();
        let mut record = guard
            .clone()
            .ok_or("native update operation is unavailable")?;
        change(&mut record)?;
        persist(&record)?;
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
    if record.recover(
        codeterm_build_info::version(),
        codeterm_build_info::sha(),
        &client.connection,
        &client.ready,
    ) {
        if let Err(error) = persist_at(&client.ready.identity.data, &record) {
            log::warn!("[updater] could not persist recovered native handoff: {error}");
        }
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
        if codeterm_update::native_handoff::validate_apply(&request, &self.ready, &self.manifest)
            .is_err()
        {
            return Err(NativeHostRejection::ProtocolMismatch);
        }
        let operation = self.app.state::<Arc<UpdateOperation>>().inner().clone();
        {
            let guard = operation.record.lock();
            if let Some(record) = guard
                .as_ref()
                .filter(|record| record.request.operation_id == request.operation_id)
            {
                return if record.request == request && record.options == options {
                    Ok(record.result.clone())
                } else {
                    Err(NativeHostRejection::ProtocolMismatch)
                };
            }
        }
        let lease = single_flight::global()
            .try_acquire()
            .ok_or(NativeHostRejection::QueueFull)?;
        let state = self.app.state::<UpdateStateHandle>().inner().clone();
        let method = state.lock().install_method.clone();
        if !codeterm_update::native_handoff::supports_native_apply(&method) {
            return Err(NativeHostRejection::Unsupported);
        }
        let record = NativeUpdateHandoff::new(request, options.clone(), self.connection.clone());
        let admitted = record.result.clone();
        let staging = operation.clone();
        tokio::task::spawn_blocking(move || {
            persist(&record)?;
            *staging.record.lock() = Some(record);
            staging.changed.send_replace(1);
            Ok::<_, String>(())
        })
        .await
        .map_err(|_| NativeHostRejection::HostRejected)?
        .map_err(|_| NativeHostRejection::HostRejected)?;
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
            let start = operation.clone();
            let outcome = async {
                tokio::task::spawn_blocking(move || {
                    start.mutate(|record| record.advance(Phase::Downloading))
                })
                .await
                .map_err(|e| e.to_string())??;
                single_flight::apply_with_lease(
                    lease,
                    &host,
                    codeterm_update::apply::ApplyOptions {
                        channel: options.channel,
                        manifest_url: options.manifest_url,
                        install_method: method,
                        initiating_tab_id: options.initiating_tab_id,
                    },
                )
                .await
            }
            .await;
            let phase = match outcome {
                Ok(codeterm_update::apply::ApplyResult::NoUpdate) => Phase::NoUpdate,
                Ok(codeterm_update::apply::ApplyResult::Restarting) => return,
                Err(message) => {
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
        self.prepare_update(version).await?;
        let operation = self.operation.clone();
        tokio::task::spawn_blocking(move || {
            operation.mutate(|record| record.advance(Phase::Relaunching))
        })
        .await
        .map_err(|e| e.to_string())?
    }
}
