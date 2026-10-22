use codeterm_service_core::service::ServiceState;
use codeterm_types::{
    native_host::{NativeHostCompletion, NativeHostOperation, NativeHostResult},
    native_update::{
        apply::{NativeUpdateApplyOptions, NativeUpdateApplyPhase as Phase},
        NativeUpdateRequest, NativeUpdateResult,
    },
};
use codeterm_update::{
    application_host::{async_trait, ApplicationHost},
    apply::{ApplyOptions, ApplyResult, DebianElevationStatus, DebianPreflight, DebianUpdatePlan},
    UpdatePhase, UpdateStateHandle,
};
use parking_lot::RwLock;
use std::{path::PathBuf, sync::Arc};

type Endpoint = crate::native_host::NativeHostEndpoint<NativeUpdateRequest, NativeUpdateResult>;
pub(crate) type NativeEndpointSlot = Arc<RwLock<Option<Endpoint>>>;

pub(crate) struct NativeApplicationHost {
    pub native: NativeEndpointSlot,
    pub service: ServiceState,
    pub update_state: UpdateStateHandle,
}

#[async_trait]
impl ApplicationHost for NativeApplicationHost {
    fn available(&self) -> bool {
        self.native
            .read()
            .as_ref()
            .is_some_and(|endpoint| endpoint.connection().is_some())
    }

    fn restart(&self) -> Result<(), String> {
        Err("native_capability_unavailable".into())
    }
    fn exit(&self, _: i32) -> Result<(), String> {
        Err("native_capability_unavailable".into())
    }
    async fn copy_image_to_clipboard(&self, _: String) -> Result<(), String> {
        Err("native_capability_unavailable".into())
    }
    fn debian_preflight(&self) -> DebianPreflight {
        DebianPreflight {
            status: DebianElevationStatus::ElevationRequired,
            reason: Some("Use the package-managed installation command.".into()),
        }
    }
    async fn check_stage_deb_update(&self, _: &ApplyOptions) -> Result<DebianUpdatePlan, String> {
        Err("native_capability_unavailable".into())
    }
    async fn install_staged_deb(
        &self,
        _: String,
        _: PathBuf,
        _: Option<String>,
    ) -> Result<(), String> {
        Err("native_capability_unavailable".into())
    }

    async fn check_and_apply(&self, options: ApplyOptions) -> Result<ApplyResult, String> {
        let endpoint = self
            .native
            .read()
            .clone()
            .ok_or("native_capability_unavailable")?;
        let connection = endpoint
            .connection()
            .ok_or("native_capability_unavailable")?;
        let request = endpoint.update_request();
        let operation_id = request.operation_id;
        let tab = options.initiating_tab_id.clone();
        let service = self.service.clone();
        let prepare_tab = tab.clone();
        tokio::task::spawn_blocking(move || {
            if let Some(tab) = prepare_tab.as_deref() {
                service.request_turn_resume_for_caller(
                    tab,
                    tab,
                    codeterm_service_core::service::turn_resume::DEFAULT_CONTINUE,
                )?;
            }
            codeterm_commands::update::shutdown::persist_session(&service)
        })
        .await
        .map_err(|e| e.to_string())??;
        let mut withdraw = false;
        let outcome = async {
            let mut completion = endpoint.request_update_on_connection(&connection, NativeHostOperation::NativeUpdate {
                request, apply: Some(NativeUpdateApplyOptions { channel: options.channel, manifest_url: options.manifest_url, initiating_tab_id: options.initiating_tab_id }),
            }).await;
            let mut sequence = 0;
            loop {
                let result = match completion {
                    NativeHostCompletion::Completed(NativeHostResult::NativeUpdateApply(result)) => result,
                    NativeHostCompletion::Rejected(reason) => { withdraw = sequence == 0; return Err(format!("native update refused: {reason:?}")); },
                    NativeHostCompletion::Uncertain(reason) => return Err(format!("native update outcome uncertain; execution will not be replayed: {reason:?}")),
                    _ => return Err("invalid native updater reply".into()),
                };
                if result.operation_id != operation_id || result.connection != connection || result.sequence <= sequence {
                    return Err("invalid native updater operation or sequence".into());
                }
                sequence = result.sequence;
                match result.phase {
                    Phase::NoUpdate => { withdraw = true; return Ok(ApplyResult::NoUpdate); },
                    Phase::Failed { message } => { withdraw = true; return Err(message); },
                    Phase::Relaunching => self.update_state.lock().set_phase(UpdatePhase::Applying),
                    Phase::Verified { .. } => return Err("unexpected replacement result on the predecessor connection".into()),
                    Phase::Admitted => self.update_state.lock().set_phase(UpdatePhase::Checking),
                    Phase::Downloading => self.update_state.lock().set_phase(UpdatePhase::Downloading),
                    Phase::Applying => self.update_state.lock().set_phase(UpdatePhase::Applying),
                }
                completion = endpoint.request_update_on_connection(&connection, NativeHostOperation::ObserveNativeUpdate { operation_id, after_sequence: sequence }).await;
            }
        }.await;
        if withdraw {
            let service = self.service.clone();
            tokio::task::spawn_blocking(move || {
                if let Some(tab) = tab {
                    service.withdraw_turn_resume_request(
                        &tab,
                        &tab,
                        codeterm_service_core::service::turn_resume::DEFAULT_CONTINUE,
                    )?;
                    codeterm_commands::update::shutdown::persist_session(&service)?;
                }
                Ok::<_, String>(())
            })
            .await
            .map_err(|e| e.to_string())??;
        }
        outcome
    }
}

#[cfg(test)]
mod tests;
