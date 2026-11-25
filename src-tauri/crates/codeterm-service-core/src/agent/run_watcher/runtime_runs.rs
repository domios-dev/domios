use super::{cron_end, AgentRunKind, RunWatcherRegistry};
use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::TabCloseReason;
use crate::service::agent_runtime::engine;
use crate::service::role_policy::CronSettlement;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::RuntimeContext;
use std::time::Duration;

mod cron_ownership;

pub(super) struct RuntimeRun {
    nonce: String,
    kind: AgentRunKind,
    armed: bool,
    ended_turn: bool,
    completion: Option<RuntimeCompletion>,
    reported: bool,
    cancelled: tokio::sync::watch::Sender<bool>,
    prepared_input: Option<PreparedCronInput>,
}

struct RuntimeCompletion {
    outcome: CronSettlement,
    barrier: super::cron_completion::CompletionBarrier,
}

struct RuntimeRunKey<'a> {
    nonce: &'a str,
    kind: Option<&'a AgentRunKind>,
    failed_start_epoch: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct CronRunAuthority {
    pub(super) tab_id: String,
    pub(super) job_id: String,
    pub(super) run_id: String,
    pub(super) binding_nonce: Option<String>,
    pub(super) pty_id: Option<String>,
}

pub(crate) struct CronRuntimeStart {
    owner: CronRunAuthority,
    input: PreparedCronInput,
}

enum PreparedCronInput {
    Empty,
    Created { record: String, send: String },
    Admitted { record: String, send: String },
}

type NativeRejectionProof = Box<
    dyn FnOnce(
            &engine::ExpectedEnsureOutcome,
            &ServiceContext,
            &crate::multiplexer::types::Tab,
            Option<(&str, &str)>,
        ) -> bool
        + Send,
>;
type CronStartGuards<'a> = (
    parking_lot::MutexGuard<'a, std::collections::HashMap<String, CronRunAuthority>>,
    parking_lot::MutexGuard<'a, std::collections::HashMap<String, RuntimeRun>>,
);

pub(super) struct RejectedCronStart {
    start: CronRuntimeStart,
    original: crate::multiplexer::types::Tab,
    fork: Option<(String, String)>,
    proof: parking_lot::Mutex<Option<NativeRejectionProof>>,
    outcome: std::sync::Arc<parking_lot::Mutex<Option<engine::ExpectedEnsureOutcome>>>,
}

impl RejectedCronStart {
    pub(super) fn admit<'a>(
        &'a self,
        svc: &'a ServiceContext,
        mux: &crate::multiplexer::state::MultiplexerState,
        store: &crate::cron::store::CronStore,
    ) -> Result<CronStartGuards<'a>, String> {
        let owner = &self.start.owner;
        let current = mux
            .get_tab(owner.tab_id())
            .filter(|_| owner.matches(mux))
            .ok_or("Original cron generation is absent or replaced")?;
        if !crate::service::messenger::switch_handoff::same_binding(&self.original, current) {
            return Err("Original cron launch context changed".into());
        }
        if !input_is_unwritten(svc, current, &self.start.input) {
            return Err("Original cron input admission changed".into());
        }
        if !active_start_receipt(store, owner) {
            return Err("Original cron receipt is no longer an active unwritten start".into());
        }
        let bindings = svc.run_watcher.cron_bindings.lock();
        if bindings.get(owner.tab_id()) != Some(owner) {
            return Err("Original cron binding changed".into());
        }
        let runs = svc.run_watcher.runtime_runs.lock();
        if !runs
            .get(owner.tab_id())
            .is_some_and(|run| unwritten_start(run, owner))
        {
            return Err("Original cron watcher changed".into());
        }
        if engine::for_tab(owner.tab_id()).is_some() {
            return Err("A runtime Session now owns this tab".into());
        }
        let mut proof = self.proof.lock();
        let outcome = self.outcome.lock();
        let outcome = outcome
            .as_ref()
            .ok_or("Native original observation was released")?;
        if !proof.take().is_some_and(|proof| {
            proof(
                outcome,
                svc,
                &self.original,
                self.fork
                    .as_ref()
                    .map(|(thread, turn)| (thread.as_str(), turn.as_str())),
            )
        }) {
            return Err("Native never-acquired proof is absent, consumed or no longer valid; no terminal resubmission is authorized".into());
        }
        Ok((bindings, runs))
    }
}

fn unwritten_start(run: &RuntimeRun, owner: &CronRunAuthority) -> bool {
    run.nonce.as_str() == owner.nonce().unwrap_or_default()
        && run.kind
            == (AgentRunKind::Cron {
                job_id: owner.job_id.clone(),
                run_id: owner.run_id.clone(),
            })
        && !run.armed
        && !run.ended_turn
        && !run.reported
        && run.completion.is_none()
        && !*run.cancelled.borrow()
}

fn active_start_receipt(store: &crate::cron::store::CronStore, owner: &CronRunAuthority) -> bool {
    store.run_is_active(owner.job_id(), owner.run_id())
        && store
            .run_receipt(owner.job_id(), owner.run_id())
            .is_some_and(|run| {
                run.tab_id.as_deref() == Some(owner.tab_id())
                    && run.session_id.is_none()
                    && run.report_delivery.is_none()
            })
}

fn input_is_unwritten(
    svc: &ServiceContext,
    current: &crate::multiplexer::types::Tab,
    input: &PreparedCronInput,
) -> bool {
    let (record, send, newly_created) = match input {
        PreparedCronInput::Empty => return current.pending_delivery.is_none(),
        PreparedCronInput::Created { record, send } => (record, send, true),
        PreparedCronInput::Admitted { record, send } => (record, send, false),
    };
    let pending = current.pending_delivery.as_ref().is_some_and(|pending| {
        pending.record_id() == record
            && pending.send_id() == Some(send.as_str())
            && pending.state() == crate::multiplexer::types::PendingDeliveryState::Pending
            && pending.attempt_provenance().is_none()
    });
    pending
        && match svc.chat_send_ledger.status(send) {
            Some(status) => {
                status.tab_id == current.id
                    && status.send_id == *send
                    && status.state == crate::service::chat_send_transaction::ChatSendState::Pending
            }
            None => newly_created,
        }
}

pub(super) struct CronCleanup {
    owner: CronRunAuthority,
    pty: Option<std::sync::Arc<crate::pty::manager::PtySession>>,
    gate: std::sync::Arc<parking_lot::Mutex<()>>,
    pad: Option<crate::service::agent_pad::CronPadLease>,
}

pub(crate) struct CronCancellation<'a> {
    registry: &'a RunWatcherRegistry,
    job: String,
}
impl Drop for CronCancellation<'_> {
    fn drop(&mut self) {
        self.registry.cron_cancelling.lock().remove(&self.job);
    }
}

impl CronRunAuthority {
    pub(crate) fn attach_claimed_pty(
        &self,
        mux: &crate::multiplexer::state::MultiplexerState,
        selected: &crate::service::pty_ownership::ClaimedPty<'_>,
    ) -> Result<Self, String> {
        if self.pty_id.is_some() || !self.matches(mux) || selected.tab_id() != self.tab_id {
            return Err("Cron original launch changed before PTY attachment".into());
        }
        Ok(Self {
            pty_id: Some(selected.pty_id().into()),
            ..self.clone()
        })
    }
    pub(crate) fn tab_id(&self) -> &str {
        &self.tab_id
    }
    pub(crate) fn job_id(&self) -> &str {
        &self.job_id
    }
    pub(crate) fn run_id(&self) -> &str {
        &self.run_id
    }
    pub(crate) fn nonce(&self) -> Option<&str> {
        self.binding_nonce.as_deref()
    }
    pub(crate) fn pty_id(&self) -> Option<&str> {
        self.pty_id.as_deref()
    }
    pub(crate) fn matches(&self, mux: &crate::multiplexer::state::MultiplexerState) -> bool {
        mux.get_tab(&self.tab_id).is_some_and(|tab| {
            tab.session_binding_nonce == self.binding_nonce && tab.pty_id == self.pty_id
        })
    }
}

impl RunWatcherRegistry {
    pub(super) fn retire_finished_watcher(&self, state: &super::RunState) {
        if matches!(state.kind, AgentRunKind::Cron { .. }) {
            return;
        }
        let mut runs = self.runs.lock();
        if runs
            .get(&state.pty_id)
            .is_some_and(|current| std::ptr::eq(current.as_ref(), state))
        {
            runs.remove(&state.pty_id);
        }
    }

    pub(crate) fn register_cron(
        &self,
        svc: ServiceContext,
        owner: &CronRunAuthority,
        timeout: u64,
        resume: Option<String>,
    ) -> Result<(), String> {
        self.register_owned(
            svc,
            owner.pty_id.clone().ok_or("Cron launch has no PTY")?,
            owner.tab_id.clone(),
            AgentRunKind::Cron {
                job_id: owner.job_id.clone(),
                run_id: owner.run_id.clone(),
            },
            timeout,
            resume,
            Some(owner),
        )
        .map(|_| ())
    }

    pub(super) fn original_cron_registration_allowed(
        &self,
        owner: Option<&CronRunAuthority>,
        mux: &crate::multiplexer::state::MultiplexerState,
        state: &super::RunState,
    ) -> bool {
        !matches!(state.kind, AgentRunKind::Cron { .. })
            || owner.is_some_and(|owner| {
                owner.tab_id == state.tab_id
                    && owner.pty_id.as_deref() == Some(state.pty_id.as_str())
                    && owner.binding_nonce == state.binding_nonce
                    && owner.matches(mux)
                    && self.cron_bindings.lock().get(&state.tab_id) == Some(owner)
            })
    }

    pub(super) fn pty_cron_registration_allowed(
        &self,
        store: Option<&crate::cron::store::CronStore>,
        mux: &crate::multiplexer::state::MultiplexerState,
        state: &super::RunState,
    ) -> bool {
        let AgentRunKind::Cron { job_id, run_id } = &state.kind else {
            return true;
        };
        store.is_some_and(|store| store.run_is_active(job_id, run_id))
            && self
                .cron_bindings
                .lock()
                .get(&state.tab_id)
                .is_some_and(|owner| {
                    owner.job_id == *job_id
                        && owner.run_id == *run_id
                        && owner.pty_id.as_deref() == Some(state.pty_id.as_str())
                        && owner.binding_nonce == state.binding_nonce
                        && owner.matches(mux)
                })
    }

    #[cfg(any(test, feature = "test-support"))]
    pub(crate) fn capture_cron_launch(
        &self,
        svc: &ServiceContext,
        linkage: &crate::service::spawn::RuntimeOwnerLinkage,
        tab: &str,
        nonce: &str,
        pty: Option<&str>,
    ) -> Result<CronRunAuthority, String> {
        if pty.is_some() {
            return Err("Cron PTY authority requires the original claim attachment".into());
        }
        let crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id } = linkage else {
            return Err("Launch is not a cron run".into());
        };
        let owner = CronRunAuthority {
            tab_id: tab.into(),
            job_id: job_id.clone(),
            run_id: run_id.clone(),
            binding_nonce: Some(nonce.into()),
            pty_id: pty.map(str::to_string),
        };
        let mux = svc.mux.read();
        if !owner.matches(&mux) {
            return Err("Cron launch changed before reservation".into());
        }
        self.retain_cron_cleanup(&owner, None);
        Ok(owner)
    }

    pub(crate) fn runtime_start_owner(&self, tab: &str, nonce: &str) -> Option<AgentRunKind> {
        self.runtime_runs
            .lock()
            .get(tab)
            .filter(|run| run.nonce == nonce)
            .map(|run| run.kind.clone())
    }

    pub(crate) fn runtime_start_result(
        &self,
        svc: &ServiceContext,
        tab: &str,
        nonce: &str,
        kind: &AgentRunKind,
        error: &crate::service::agent_runtime::model::RuntimeFailure,
    ) -> Option<cron_end::CronEndHandle> {
        use crate::service::agent_runtime::model::RuntimeFailure;
        if matches!(
            error,
            RuntimeFailure::Superseded
                | RuntimeFailure::SignInRequired
                | RuntimeFailure::WrittenAwaitingEvidence
        ) {
            return None;
        }
        let failed_start_epoch = if matches!(error, RuntimeFailure::Deadline) {
            Some(ended_unbound_start(svc, tab, nonce)?)
        } else {
            None
        };
        let current = svc
            .mux
            .read()
            .get_tab(tab)
            .is_some_and(|target| target.session_binding_nonce.as_deref() == Some(nonce));
        if !current {
            return None;
        }
        self.finish_runtime_owned(
            svc,
            tab,
            RuntimeRunKey {
                nonce,
                kind: Some(kind),
                failed_start_epoch,
            },
            false,
            false,
            format!("Runtime startup failed: {error}"),
        )
    }

    fn cron_start_is_unwritten(
        &self,
        svc: &ServiceContext,
        owner: &CronRunAuthority,
        original: Option<&crate::multiplexer::types::Tab>,
    ) -> bool {
        if owner.pty_id().is_some() || self.cron_bindings.lock().get(owner.tab_id()) != Some(owner)
        {
            return false;
        }
        let runs = self.runtime_runs.lock();
        let eligible = runs.get(owner.tab_id()).is_some_and(|run| {
            run.nonce.as_str() == owner.nonce().unwrap_or_default()
                && run.kind
                    == (AgentRunKind::Cron {
                        job_id: owner.job_id.clone(),
                        run_id: owner.run_id.clone(),
                    })
                && !run.armed
                && !run.ended_turn
                && !run.reported
                && run.completion.is_none()
                && !*run.cancelled.borrow()
        });
        drop(runs);
        if !eligible {
            return false;
        }
        if let Some(pending) = original.and_then(|tab| tab.pending_delivery.as_ref()) {
            if pending.state() != crate::multiplexer::types::PendingDeliveryState::Pending
                || pending.attempt_provenance().is_some()
            {
                return false;
            }
            if !pending
                .send_id()
                .and_then(|id| svc.chat_send_ledger.status(id))
                .is_some_and(|status| {
                    status.tab_id == owner.tab_id
                        && status.state
                            == crate::service::chat_send_transaction::ChatSendState::Pending
                })
            {
                return false;
            }
        }
        let Some(cron) = svc.try_ext::<crate::CronState>() else {
            return false;
        };
        let store = cron.lock();
        store.run_is_active(owner.job_id(), owner.run_id())
            && store
                .run_receipt(owner.job_id(), owner.run_id())
                .is_some_and(|run| {
                    run.tab_id.as_deref() == Some(owner.tab_id())
                        && run.session_id.is_none()
                        && run.report_delivery.is_none()
                })
    }

    pub(crate) fn seal_prepared_cron_input(
        &self,
        svc: &ServiceContext,
        owner: &CronRunAuthority,
        created: Option<crate::multiplexer::types::PendingDeliveryRecord>,
    ) -> Result<(), String> {
        let mut mux = svc.mux.write();
        let current = mux
            .get_tab(owner.tab_id())
            .filter(|_| owner.matches(&mux))
            .ok_or("Original cron input generation changed")?;
        if current.pending_delivery.is_some() {
            return Err("Existing pending input cannot acquire creation authority".into());
        }
        if !active_start_receipt(
            &svc.try_ext::<crate::CronState>()
                .ok_or("Cron state absent")?
                .lock(),
            owner,
        ) {
            return Err("Original cron input receipt is no longer active".into());
        }
        let mut runs = self.runtime_runs.lock();
        let run = runs
            .get_mut(owner.tab_id())
            .filter(|run| unwritten_start(run, owner))
            .ok_or("Original cron watcher changed before input preparation")?;
        if run.prepared_input.is_some() {
            return Err("Cron initial input preparation was already sealed".into());
        }
        let input = match created.as_ref() {
            Some(record) => {
                let send = record
                    .send_id()
                    .ok_or("New cron input has no original send ID")?;
                if record.state() != crate::multiplexer::types::PendingDeliveryState::Pending
                    || record.attempt_provenance().is_some()
                {
                    return Err("New cron input was already claimed".into());
                }
                PreparedCronInput::Created {
                    record: record.record_id().to_string(),
                    send: send.into(),
                }
            }
            None => PreparedCronInput::Empty,
        };
        mux.get_tab_mut(owner.tab_id())
            .ok_or("Original cron input tab disappeared")?
            .pending_delivery = created;
        run.prepared_input = Some(input);
        Ok(())
    }

    pub(crate) fn capture_cron_runtime_start(
        &self,
        svc: &ServiceContext,
        owner: &CronRunAuthority,
        original: &crate::multiplexer::types::Tab,
    ) -> Option<CronRuntimeStart> {
        let mux = svc.mux.read();
        let current = mux
            .get_tab(owner.tab_id())
            .filter(|_| owner.matches(&mux))?;
        let bound = self.cron_bindings.lock().get(owner.tab_id()) == Some(owner);
        if !bound || !active_start_receipt(&svc.try_ext::<crate::CronState>()?.lock(), owner) {
            return None;
        }
        let mut runs = self.runtime_runs.lock();
        let run = runs
            .get_mut(owner.tab_id())
            .filter(|run| unwritten_start(run, owner))?;
        let input = match run.prepared_input.take() {
            Some(prepared) => prepared,
            None => {
                let pending = original.pending_delivery.as_ref()?;
                let send = pending.send_id()?;
                let status = svc.chat_send_ledger.status(send)?;
                if status.tab_id != owner.tab_id
                    || status.state != crate::service::chat_send_transaction::ChatSendState::Pending
                {
                    return None;
                }
                PreparedCronInput::Admitted {
                    record: pending.record_id().to_string(),
                    send: send.into(),
                }
            }
        };
        if !input_is_unwritten(svc, current, &input) {
            return None;
        }
        Some(CronRuntimeStart {
            owner: owner.clone(),
            input,
        })
    }

    pub(crate) fn runtime_rejected_before_acquisition(
        &self,
        svc: &ServiceContext,
        retained: CronRuntimeStart,
        original: &crate::multiplexer::types::Tab,
        fork: Option<(&str, &str)>,
        mut outcome: engine::ExpectedEnsureOutcome,
    ) -> Result<std::sync::Arc<engine::Session>, crate::service::agent_runtime::model::RuntimeFailure>
    {
        let Some(proof) = outcome.take_rejection() else {
            return outcome.into_result();
        };
        let owner = retained.owner.clone();
        if original.id != owner.tab_id
            || original.session_binding_nonce != owner.binding_nonce
            || original.pty_id.is_some()
            || !owner.matches(&svc.mux.read())
        {
            return outcome.into_result();
        }
        let outcome = std::sync::Arc::new(parking_lot::Mutex::new(Some(outcome)));
        let native: NativeRejectionProof = Box::new(move |outcome, svc, original, fork| {
            proof.matches_original(outcome, svc, original, fork)
        });
        let admission = RejectedCronStart {
            start: retained,
            original: original.clone(),
            fork: fork.map(|(thread, turn)| (thread.into(), turn.into())),
            proof: parking_lot::Mutex::new(Some(native)),
            outcome: outcome.clone(),
        };
        if let Some(handle) = cron_end::finish_rejected(
            svc,
            cron_end::EndedCronRun {
                tab_id: owner.tab_id(),
                job_id: owner.job_id(),
                run_id: owner.run_id(),
                success: false,
                summary: "Runtime startup rejected before original session acquisition".into(),
                raw_output: "",
                session_id: None,
                cwd: None,
                binding_nonce: owner.nonce(),
                pty_id: None,
                failed_start_epoch: None,
            },
            admission,
        ) {
            let _ = handle.wait_for_terminal();
        }
        let result = outcome
            .lock()
            .take()
            .expect("Original native outcome is consumed once")
            .into_result();
        result
    }

    pub(crate) fn runtime_ended_start_result(
        &self,
        svc: &ServiceContext,
        owner: &CronRunAuthority,
        original: &crate::multiplexer::types::Tab,
        error: &crate::service::agent_runtime::model::RuntimeFailure,
    ) -> Option<cron_end::CronEndHandle> {
        use crate::service::agent_runtime::model::RuntimeFailure;
        if matches!(
            error,
            RuntimeFailure::Superseded
                | RuntimeFailure::SignInRequired
                | RuntimeFailure::WrittenAwaitingEvidence
        ) || matches!(error, RuntimeFailure::Refused(reason) if reason.starts_with("account_provisioning_busy:") || reason.starts_with("launch_account_supervision_unconfirmed:"))
            || !self.cron_start_is_unwritten(svc, owner, Some(original))
        {
            return None;
        }
        let nonce = owner.nonce()?;
        let epoch = ended_unbound_start(svc, owner.tab_id(), nonce)?;
        let kind = AgentRunKind::Cron {
            job_id: owner.job_id.clone(),
            run_id: owner.run_id.clone(),
        };
        self.finish_runtime_owned(
            svc,
            owner.tab_id(),
            RuntimeRunKey {
                nonce,
                kind: Some(&kind),
                failed_start_epoch: Some(epoch),
            },
            false,
            false,
            format!("Runtime startup failed: {error}"),
        )
    }

    pub(crate) fn has_runtime_run(&self, tab: &str) -> bool {
        self.runtime_runs.lock().contains_key(tab)
    }

    pub(crate) fn register_runtime(
        &self,
        svc: &ServiceContext,
        tab: &str,
        kind: AgentRunKind,
        timeout_secs: u64,
    ) -> Result<(), String> {
        let mux = svc.mux.read();
        let nonce = {
            let target = mux.get_tab(tab).ok_or("Runtime run tab absent")?;
            if !target
                .agent_type
                .as_deref()
                .is_some_and(|provider| engine::requested(svc, provider))
            {
                return Err("Runtime run requires agentRuntime".into());
            }
            target
                .session_binding_nonce
                .clone()
                .ok_or("Runtime run binding absent")?
        };
        let (cancelled, mut completion) = tokio::sync::watch::channel(false);
        {
            let cron = svc.try_ext::<crate::CronState>().map(|cron| cron.lock());
            if let AgentRunKind::Cron { job_id, run_id } = &kind {
                let active = cron.as_ref().is_some_and(|store| {
                    store.run_is_active(job_id, run_id)
                        && store
                            .run_receipt(job_id, run_id)
                            .is_some_and(|run| run.tab_id.as_deref() == Some(tab))
                        && self.cron_binding(tab, job_id, run_id).is_some_and(|owner| {
                            owner.binding_nonce.as_deref() == Some(nonce.as_str())
                                && owner.pty_id.is_none()
                                && owner.matches(&mux)
                        })
                });
                if !active {
                    return Err("Runtime cron run is no longer active".into());
                }
            }
            let mut runs = self.runtime_runs.lock();
            if let Some(existing) = runs.get(tab) {
                return if existing.nonce == nonce && existing.kind == kind {
                    Ok(())
                } else {
                    Err("Runtime run ownership changed".into())
                };
            }
            runs.insert(
                tab.into(),
                RuntimeRun {
                    prepared_input: None,
                    nonce: nonce.clone(),
                    kind: kind.clone(),
                    armed: false,
                    ended_turn: false,
                    completion: matches!(&kind, AgentRunKind::StepRun { .. }).then(|| {
                        RuntimeCompletion {
                            outcome: CronSettlement::success("agent turn completed"),
                            barrier: super::cron_completion::CompletionBarrier::default(),
                        }
                    }),
                    reported: false,
                    cancelled,
                },
            );
        }
        drop(mux);
        {
            let owner = svc.clone();
            let tab = tab.to_string();
            crate::async_runtime::spawn(async move {
                if runtime_deadline(
                    &mut completion,
                    tokio::time::sleep(Duration::from_secs(timeout_secs)),
                )
                .await
                {
                    let watcher = owner.run_watcher.clone();
                    let _ = tokio::task::spawn_blocking(move || {
                        let _ = watcher.finish_runtime_owned(
                            &owner,
                            &tab,
                            RuntimeRunKey {
                                nonce: &nonce,
                                kind: Some(&kind),
                                failed_start_epoch: None,
                            },
                            false,
                            true,
                            format!("hard timeout after {timeout_secs}s"),
                        );
                    })
                    .await;
                }
            });
        }
        Ok(())
    }

    pub(crate) fn arm_runtime(&self, tab: &str, nonce: &str) {
        if let Some(run) = self
            .runtime_runs
            .lock()
            .get_mut(tab)
            .filter(|run| run.nonce == nonce)
        {
            run.armed = true;
        }
    }

    pub fn request_runtime_cron_completion(
        &self,
        svc: &ServiceContext,
        tab: &str,
        run_id: &str,
        outcome: &CronSettlement,
    ) -> Option<Result<(), String>> {
        if let Some(status) = self.resume_cron_finalizer(svc, tab, run_id) {
            return Some(outcome.confirm_recorded(&status));
        }
        let explicit_finish = svc.run_completion_for_tab(tab)
            == crate::service::role_policy::RunCompletion::ExplicitFinish;
        let messages = runtime_messages(tab);
        let nonce = svc
            .mux
            .read()
            .get_tab(tab)
            .and_then(|target| target.session_binding_nonce.clone());
        let mut runs = self.runtime_runs.lock();
        let run = runs.get_mut(tab)?;
        if !matches!(&run.kind, AgentRunKind::Cron { run_id: own, .. } if own == run_id) {
            return Some(Err("Runtime cron run ownership changed".into()));
        }
        if nonce.as_deref() != Some(run.nonce.as_str()) {
            return Some(Err("Runtime cron binding changed".into()));
        }
        if run.ended_turn && run.completion.is_none() && !explicit_finish {
            return Some(Err("Cron turn already ended".into()));
        }
        let completion = run.completion.get_or_insert_with(|| RuntimeCompletion {
            outcome: outcome.clone(),
            barrier: super::cron_completion::CompletionBarrier {
                stopped: false,
                assistant_ids: messages.iter().map(|message| message.id.clone()).collect(),
            },
        });
        Some(completion.outcome.confirm_outcome(outcome))
    }

    pub fn runtime_worker_reported(&self, tab: &str, worker: &str) -> bool {
        let mut runs = self.runtime_runs.lock();
        let Some(run) = runs.get_mut(tab).filter(|run| {
            matches!(&run.kind,
            AgentRunKind::MemWorker { worker_id } if worker_id == worker)
        }) else {
            return false;
        };
        run.reported = true;
        let _ = run.cancelled.send(true);
        true
    }

    pub(crate) fn runtime_turn_completed(&self, svc: &ServiceContext, context: &RuntimeContext) {
        if !current_runtime_binding(svc, context) {
            return;
        }
        {
            let mut runs = self.runtime_runs.lock();
            let Some(run) = runs
                .get_mut(&context.tab_id)
                .filter(|run| run.nonce == context.binding_nonce && run.armed)
            else {
                return;
            };
            run.ended_turn = true;
            if let Some(completion) = run.completion.as_mut() {
                completion.barrier.stopped = true;
            }
        }
        self.check_runtime_completion(svc, context);
    }

    pub(crate) fn check_runtime_completion(&self, svc: &ServiceContext, context: &RuntimeContext) {
        if context.active_turn_id.is_some() || !current_runtime_binding(svc, context) {
            return;
        }
        let messages = runtime_messages(&context.tab_id);
        let outcome = {
            let runs = self.runtime_runs.lock();
            let Some(run) = runs
                .get(&context.tab_id)
                .filter(|run| run.nonce == context.binding_nonce && run.ended_turn)
            else {
                return;
            };
            match &run.completion {
                Some(completion) if !completion.barrier.ready(&messages) => return,
                Some(completion) => completion.outcome.clone(),
                None if svc.run_completion_for_tab(&context.tab_id)
                    == crate::service::role_policy::RunCompletion::ExplicitFinish =>
                {
                    return
                }
                None => CronSettlement::success("(agent returned to idle)"),
            }
        };
        let _ = self.finish_runtime(
            svc,
            &context.tab_id,
            &context.binding_nonce,
            outcome.success,
            false,
            outcome.summary,
        );
    }

    pub(crate) fn runtime_process_exited(
        &self,
        svc: &ServiceContext,
        context: &RuntimeContext,
        reason: String,
    ) {
        self.runtime_turn_failed(svc, context, reason);
    }

    pub(crate) fn runtime_turn_failed(
        &self,
        svc: &ServiceContext,
        context: &RuntimeContext,
        reason: String,
    ) {
        if current_runtime_binding(svc, context) {
            let _ = self.finish_runtime(
                svc,
                &context.tab_id,
                &context.binding_nonce,
                false,
                false,
                reason,
            );
        }
    }

    fn finish_runtime(
        &self,
        svc: &ServiceContext,
        tab: &str,
        nonce: &str,
        success: bool,
        timed_out: bool,
        summary: String,
    ) -> Option<cron_end::CronEndHandle> {
        self.finish_runtime_owned(
            svc,
            tab,
            RuntimeRunKey {
                nonce,
                kind: None,
                failed_start_epoch: None,
            },
            success,
            timed_out,
            summary,
        )
    }

    fn finish_runtime_owned(
        &self,
        svc: &ServiceContext,
        tab: &str,
        key: RuntimeRunKey<'_>,
        success: bool,
        timed_out: bool,
        summary: String,
    ) -> Option<cron_end::CronEndHandle> {
        let nonce = key.nonce;
        let cron_kind = self
            .runtime_runs
            .lock()
            .get(tab)
            .filter(|run| run.nonce == nonce && key.kind.is_none_or(|kind| kind == &run.kind))
            .and_then(|run| match &run.kind {
                AgentRunKind::Cron { job_id, run_id } => Some((job_id.clone(), run_id.clone())),
                _ => None,
            });
        if let Some((job_id, run_id)) = cron_kind {
            let (session, cwd) = {
                let mux = svc.mux.read();
                let target = mux.get_tab(tab).filter(|target| {
                    target.session_binding_nonce.as_deref() == Some(nonce)
                        && target.pty_id.is_none()
                });
                (
                    target.and_then(|target| target.agent_session_id.clone()),
                    target.and_then(|target| target.cwd.clone()),
                )
            };
            let (output, complete_output) = engine::for_tab(tab).filter(|session| session.context.lock().binding_nonce == nonce)
                .filter(|_| svc.mux.read().get_tab(tab).is_some_and(|target| target.session_binding_nonce.as_deref() == Some(nonce)))
                .map(|session| session.cache.lock().terminal_evidence()).unwrap_or_else(|| ("PARTIAL runtime cache evidence unavailable; full transcript bytecount/digest unavailable".into(), false));
            return cron_end::finish_projected(
                svc,
                cron_end::EndedCronRun {
                    tab_id: tab,
                    job_id: &job_id,
                    run_id: &run_id,
                    success,
                    summary,
                    raw_output: &output,
                    session_id: session.as_deref(),
                    cwd: cwd.as_deref(),
                    binding_nonce: Some(nonce),
                    pty_id: None,
                    failed_start_epoch: key.failed_start_epoch,
                },
                complete_output,
            );
        }
        let run = {
            let mut runs = self.runtime_runs.lock();
            if !runs.get(tab).is_some_and(|run| {
                run.nonce == nonce
                    && key.kind.is_none_or(|kind| kind == &run.kind)
                    && !(timed_out && run.reported)
            }) {
                return None;
            }
            let Some(run) = runs.remove(tab) else {
                return None;
            };
            run
        };
        let _ = run.cancelled.send(true);
        let (session, cwd, current) = {
            let mux = svc.mux.read();
            let target = mux.get_tab(tab);
            (
                target.and_then(|target| target.agent_session_id.clone()),
                target.and_then(|target| target.cwd.clone()),
                target.is_none_or(|target| target.session_binding_nonce.as_deref() == Some(nonce)),
            )
        };
        if !current {
            return None;
        }
        let output = serde_json::to_string(&runtime_messages(tab)).unwrap_or_default();
        log::debug!("[run-watcher.runtime] end tab={tab} success={success} timed_out={timed_out} reported={}", run.reported);
        match run.kind {
            AgentRunKind::StepRun { run_id } => crate::service::step_agent_run::finish(
                svc,
                &run_id,
                success,
                summary,
                Ok(runtime_messages(tab)),
            ),
            AgentRunKind::Cron { job_id, run_id } => {
                return cron_end::finish(
                    svc,
                    cron_end::EndedCronRun {
                        tab_id: tab,
                        job_id: &job_id,
                        run_id: &run_id,
                        success,
                        summary,
                        raw_output: &output,
                        session_id: session.as_deref(),
                        cwd: cwd.as_deref(),
                        binding_nonce: Some(nonce),
                        pty_id: None,
                        failed_start_epoch: None,
                    },
                )
            }
            AgentRunKind::MemWorker { worker_id } => {
                if let Ok(path) = crate::mem::workers::save_worker_log(&worker_id, &output) {
                    svc.workers
                        .set_log_path(&worker_id, path.to_string_lossy().into_owned());
                }
                if let Some(session) = session {
                    svc.workers.set_run_session(&worker_id, session, cwd);
                }
                if !run.reported {
                    let end = if timed_out {
                        codeterm_mem::dream_status::ExecutionEnd::TimedOut {
                            detail: summary.clone(),
                        }
                    } else {
                        codeterm_mem::dream_status::ExecutionEnd::ProcessEnded {
                            detail: if success {
                                format!("execution ended without mem worker report: {summary}")
                            } else {
                                summary.clone()
                            },
                        }
                    };
                    let _ = svc
                        .workers
                        .finish_execution(&worker_id, Some(summary.clone()), end);
                    if timed_out {
                        crate::mem::dream_runs::finalize_dream_at_deadline(
                            svc, &worker_id, &summary,
                        );
                    }
                }
                let _ = svc.close_tab_with_reason(tab, TabCloseReason::AgentRunEnded);
                if let Some(info) = svc.workers.get(&worker_id) {
                    svc.ui.emit(
                        "graph-worker-done",
                        serde_json::to_value(info).unwrap_or_default(),
                    );
                }
            }
        }
        None
    }
}

fn ended_unbound_start(svc: &ServiceContext, tab: &str, nonce: &str) -> Option<u64> {
    let session = engine::for_tab(tab)?;
    let _permit = session.gate.try_acquire().ok()?;
    unbound_start_epoch(svc, &session, tab, nonce)
}

pub(super) fn unbound_start_epoch(
    svc: &ServiceContext,
    session: &std::sync::Arc<engine::Session>,
    tab: &str,
    nonce: &str,
) -> Option<u64> {
    let context = session.context.lock().clone();
    let state = session.state.lock().clone();
    let process = session.process.lock().clone();
    (context.binding_nonce == nonce
        && context.tab_id == tab
        && context.thread_id.is_none()
        && state.epoch == context.epoch
        && !session
            .thread_loaded
            .load(std::sync::atomic::Ordering::Acquire)
        && matches!(
            state.phase,
            crate::service::agent_runtime::model::RuntimePhase::Degraded { .. }
        )
        && process.is_none_or(|process| {
            process.ended.borrow().is_some() || process.client.ended().borrow().is_some()
        })
        && engine::for_tab(tab).is_some_and(|current| std::sync::Arc::ptr_eq(&current, session))
        && svc.mux.read().get_tab(tab).is_some_and(|target| {
            target.agent_type.as_deref() == Some(session.provider.as_str())
                && target.session_binding_nonce.as_deref() == Some(nonce)
                && target.agent_session_id.is_none()
        }))
    .then_some(context.epoch)
}

async fn runtime_deadline(
    completion: &mut tokio::sync::watch::Receiver<bool>,
    deadline: impl std::future::Future<Output = ()>,
) -> bool {
    if *completion.borrow() {
        return false;
    }
    tokio::select! {
        biased;
        _ = completion.changed() => false,
        _ = deadline => true,
    }
}

fn runtime_messages(tab: &str) -> Vec<ChatMsg> {
    engine::for_tab(tab)
        .map(|session| session.cache.lock().snapshot())
        .unwrap_or_default()
}

fn current_runtime_binding(svc: &ServiceContext, context: &RuntimeContext) -> bool {
    let epoch_matches = engine::for_tab(&context.tab_id).is_some_and(|session| {
        let own = session.context.lock();
        own.epoch == context.epoch
            && own.binding_nonce == context.binding_nonce
            && own.thread_id == context.thread_id
    });
    epoch_matches
        && svc.mux.read().get_tab(&context.tab_id).is_some_and(|tab| {
            tab.session_binding_nonce.as_deref() == Some(context.binding_nonce.as_str())
                && tab.agent_session_id == context.thread_id
        })
}

impl ServiceContext {
    pub(crate) fn prepare_runtime_run_watcher(
        &self,
        tab: &str,
        metadata: &crate::service::spawn::ChatBackendLaunchMetadata,
    ) -> Result<(), String> {
        let (kind, timeout) = if let Some(crate::service::spawn::RuntimeOwnerLinkage::StepRun {
            run_id,
            timeout_seconds,
        }) = &metadata.owner_linkage
        {
            (
                AgentRunKind::StepRun {
                    run_id: run_id.clone(),
                },
                *timeout_seconds,
            )
        } else if let Some(crate::service::spawn::RuntimeOwnerLinkage::CronRun { job_id, run_id }) =
            &metadata.owner_linkage
        {
            let timeout = self
                .try_ext::<crate::CronState>()
                .and_then(|cron| cron.lock().get(job_id).and_then(|job| job.timeout_seconds))
                .unwrap_or(self.configuration().memory.worker_timeout_seconds);
            (
                AgentRunKind::Cron {
                    job_id: job_id.clone(),
                    run_id: run_id.clone(),
                },
                timeout,
            )
        } else if let Some(worker_id) = &metadata.agent.worker_id {
            let intent = self
                .workers
                .get(worker_id)
                .map(|worker| worker.intent)
                .unwrap_or_default();
            (
                AgentRunKind::MemWorker {
                    worker_id: worker_id.clone(),
                },
                crate::mem::worker_deadline::worker_deadline_secs(
                    &intent,
                    &self.configuration().memory,
                ),
            )
        } else {
            return Ok(());
        };
        self.run_watcher.register_runtime(self, tab, kind, timeout)
    }
}

#[cfg(test)]
mod tests;
