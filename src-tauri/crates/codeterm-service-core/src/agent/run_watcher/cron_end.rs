use parking_lot::Mutex;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc,
};
use std::thread::JoinHandle;

use super::runtime_runs::CronRunAuthority;
use crate::cron::store::storage::{log_finished, RunFinished, TerminalRun, TerminalRunCommit};
use crate::cron::store::{CronRunStatus, CronStore};
use crate::multiplexer::types::TabCloseReason;
use crate::service::cron::CronRunEnd;
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceContext;

mod recovery;
pub(super) use recovery::Recovery;

type Deliver = Box<dyn FnOnce(&str, MessageEnvelope, &str) -> Result<(), String> + Send>;
enum DeliveryPort {
    Production,
    Injected(Deliver),
    Consumed,
}

pub(super) struct EndedCronRun<'a> {
    pub(super) tab_id: &'a str,
    pub(super) job_id: &'a str,
    pub(super) run_id: &'a str,
    pub(super) success: bool,
    pub(super) summary: String,
    pub(super) raw_output: &'a str,
    pub(super) session_id: Option<&'a str>,
    pub(super) cwd: Option<&'a str>,
    pub(super) binding_nonce: Option<&'a str>,
    pub(super) pty_id: Option<&'a str>,
    pub(super) failed_start_epoch: Option<u64>,
}

pub(super) struct Finalizer {
    owner: CronRunAuthority,
    request: Mutex<TerminalRun>,
    raw_output: String,
    raw_bytes: Option<usize>,
    known_finished: Mutex<Option<RunFinished>>,
    cancelled: AtomicBool,
    delivery_gate: Mutex<()>,
    unknown: AtomicBool,
    plain_shell: bool,
    attempt: Mutex<Option<TerminalRunCommit>>,
    deliver: Mutex<DeliveryPort>,
    running: AtomicBool,
    resume_after_allocation: AtomicBool,
    effects_started: AtomicBool,
    role_settled: AtomicBool,
    failure: Mutex<Option<String>>,
    failed_start_epoch: Option<u64>,
    rejection: Option<super::runtime_runs::RejectedCronStart>,
    phase: Mutex<FinalPhase>,
    report: Mutex<Option<(crate::service::cron::CronReportPlan, String)>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinalPhase {
    Terminal,
    Cleanup,
    Delivery,
    Report,
    Retire,
}

pub(crate) struct CronEndHandle {
    terminal: std::sync::mpsc::Receiver<Result<bool, String>>,
    thread: JoinHandle<()>,
}

impl CronEndHandle {
    pub(crate) fn wait_for_terminal(self) -> Result<bool, String> {
        let result = self
            .terminal
            .recv()
            .map_err(|_| "Cron terminal owner ended without an outcome".to_string())?;
        drop(self.thread);
        result
    }

    #[cfg(test)]
    pub(super) fn join(self) -> std::thread::Result<()> {
        self.thread.join()
    }
}

pub(super) fn finish(svc: &ServiceContext, run: EndedCronRun<'_>) -> Option<CronEndHandle> {
    start(svc, run, None)
}

#[cfg(test)]
fn finish_with<D>(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: D) -> Option<JoinHandle<()>>
where
    D: FnOnce(&str, MessageEnvelope, &str) -> Result<(), String> + Send + 'static,
{
    start(svc, run, Some(Box::new(deliver))).map(|handle| handle.thread)
}

struct Running<'a> {
    owner: &'a Finalizer,
    svc: &'a ServiceContext,
}
impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.owner.running.store(false, Ordering::Release);
        if self
            .owner
            .resume_after_allocation
            .swap(false, Ordering::AcqRel)
            && self
                .svc
                .run_watcher
                .cron_record(self.owner.owner.job_id(), self.owner.owner.run_id())
                .is_some()
        {
            restart_finalizer(self.svc, self.owner);
        }
    }
}

fn restart_finalizer(svc: &ServiceContext, retained: &Finalizer) {
    let request = retained.request.lock().clone();
    let _ = finish(
        svc,
        EndedCronRun {
            tab_id: retained.owner.tab_id(),
            job_id: &request.job_id,
            run_id: &request.run_id,
            success: request.status == CronRunStatus::Success,
            summary: request.summary,
            raw_output: &retained.raw_output,
            session_id: request.session_id.as_deref(),
            cwd: request.cwd.as_deref(),
            binding_nonce: retained.owner.nonce(),
            pty_id: retained.owner.pty_id(),
            failed_start_epoch: retained.failed_start_epoch,
        },
    );
}

fn start(
    svc: &ServiceContext,
    run: EndedCronRun<'_>,
    deliver: Option<Deliver>,
) -> Option<CronEndHandle> {
    start_gated(svc, run, deliver, None, None, true)
}

pub(super) fn finish_projected(
    svc: &ServiceContext,
    run: EndedCronRun<'_>,
    complete: bool,
) -> Option<CronEndHandle> {
    start_gated(svc, run, None, None, None, complete)
}

pub(super) fn finish_rejected(
    svc: &ServiceContext,
    run: EndedCronRun<'_>,
    admission: super::runtime_runs::RejectedCronStart,
) -> Option<CronEndHandle> {
    start_gated(svc, run, None, None, Some(admission), true)
}

struct ResponseFence {
    release: std::sync::mpsc::Receiver<()>,
    cleaned: std::sync::mpsc::Sender<Result<(), String>>,
}

pub(crate) struct ResponseRelease {
    release: std::sync::mpsc::Sender<()>,
    cleaned: std::sync::mpsc::Receiver<Result<(), String>>,
}

impl ResponseRelease {
    pub(crate) fn finish(self) -> Result<(), String> {
        let _ = self.release.send(());
        self.cleaned
            .recv()
            .map_err(|_| "Cron cleanup owner ended without a receipt".to_string())?
    }
}

pub(crate) fn finish_after_response(
    svc: &ServiceContext,
    owner: &CronRunAuthority,
    summary: String,
    raw: &str,
    session: Option<&str>,
    cwd: Option<&str>,
) -> Result<Option<ResponseRelease>, String> {
    let (release, wait) = std::sync::mpsc::channel();
    let (cleaned, receipt) = std::sync::mpsc::channel();
    let handle = start_gated(
        svc,
        EndedCronRun {
            tab_id: owner.tab_id(),
            job_id: owner.job_id(),
            run_id: owner.run_id(),
            success: true,
            summary,
            raw_output: raw,
            session_id: session,
            cwd,
            binding_nonce: owner.nonce(),
            pty_id: owner.pty_id(),
            failed_start_epoch: None,
        },
        None,
        Some(ResponseFence {
            release: wait,
            cleaned,
        }),
        None,
        true,
    );
    let Some(handle) = handle else {
        return Ok(None);
    };
    if !handle.wait_for_terminal()? {
        return Ok(None);
    }
    Ok(Some(ResponseRelease {
        release,
        cleaned: receipt,
    }))
}

fn start_gated(
    svc: &ServiceContext,
    run: EndedCronRun<'_>,
    deliver: Option<Deliver>,
    response: Option<ResponseFence>,
    rejection: Option<super::runtime_runs::RejectedCronStart>,
    complete_output: bool,
) -> Option<CronEndHandle> {
    let cron = svc.try_ext::<crate::CronState>()?.clone();
    let key = (run.job_id.to_string(), run.run_id.to_string());
    let own = CronRunAuthority {
        tab_id: run.tab_id.into(),
        job_id: run.job_id.into(),
        run_id: run.run_id.into(),
        binding_nonce: run.binding_nonce.map(str::to_string),
        pty_id: run.pty_id.map(str::to_string),
    };
    if svc.run_watcher.unknown_cron_outcome(run.run_id) {
        return None;
    }
    let finalizer = {
        let mut owners = svc.run_watcher.cron_finalizers.lock();
        if owners.len() >= recovery::MAX_OWNERS && !owners.contains_key(&key) {
            log::error!("Cron terminal owner capacity reached; original launch remains owned for explicit cancellation");
            return None;
        }
        let entry = owners.entry(key.clone()).or_insert_with(|| {
            Arc::new(Finalizer {
                owner: own.clone(),
                request: Mutex::new(TerminalRun {
                    job_id: run.job_id.into(),
                    run_id: run.run_id.into(),
                    status: if run.success {
                        CronRunStatus::Success
                    } else {
                        CronRunStatus::Failed
                    },
                    summary: recovery::bounded(&run.summary, recovery::MAX_SUMMARY),
                    session_id: run
                        .session_id
                        .filter(|value| value.len() <= 1024)
                        .map(str::to_string),
                    cwd: run
                        .cwd
                        .filter(|value| value.len() <= 4096)
                        .map(str::to_string),
                    tab_id: Some(run.tab_id.into()),
                }),
                raw_output: recovery::tail(run.raw_output),
                raw_bytes: complete_output.then_some(run.raw_output.len()),
                known_finished: Mutex::new(None),
                cancelled: AtomicBool::new(false),
                delivery_gate: Mutex::new(()),
                unknown: AtomicBool::new(false),
                plain_shell: response.is_some(),
                attempt: Mutex::new(None),
                deliver: Mutex::new(
                    deliver
                        .map(DeliveryPort::Injected)
                        .unwrap_or(DeliveryPort::Production),
                ),
                running: AtomicBool::new(false),
                resume_after_allocation: AtomicBool::new(false),
                effects_started: AtomicBool::new(false),
                role_settled: AtomicBool::new(false),
                failure: Mutex::new(None),
                failed_start_epoch: run.failed_start_epoch,
                rejection,
                phase: Mutex::new(FinalPhase::Terminal),
                report: Mutex::new(None),
            })
        });
        if entry.owner != own || entry.running.swap(true, Ordering::AcqRel) {
            return None;
        }
        entry.clone()
    };
    let svc = svc.clone();
    let (settled, terminal) = std::sync::mpsc::channel();
    let thread = codeterm_util::named_thread::spawn("cron-run-end", move || {
        let _running = Running {
            owner: &finalizer,
            svc: &svc,
        };
        let (mut response_release, mut cleaned) = match response {
            Some(fence) => (Some(fence.release), Some(fence.cleaned)),
            None => (None, None),
        };
        let result: Result<(), String> = (|| {
            if !cron.lock().run_is_active(&key.0, &key.1)
                && svc.run_watcher.cron_record(&key.0, &key.1).is_none()
            {
                let _ = settled.send(Ok(false));
                return Ok(());
            }
            if svc.run_watcher.cron_record(&key.0, &key.1).is_none() {
                svc.run_watcher
                    .reserve_cron_slot(&svc, &cron, &key.0, &key.1)?;
            }
            svc.run_watcher.retain_cron_cleanup(&finalizer.owner, None);
            svc.run_watcher.journal_cron_owner(&finalizer.owner)?;
            if cron.lock().run_is_cancelled(&key.0, &key.1) {
                finalizer.cancelled.store(true, Ordering::Release);
            }
            let start_epoch = if *finalizer.phase.lock() == FinalPhase::Terminal
                && finalizer.known_finished.lock().is_none()
                && !finalizer.cancelled.load(Ordering::Acquire)
            {
                finalizer.failed_start_epoch
            } else {
                None
            };
            let startup_session = start_epoch.and_then(|_| {
                crate::service::agent_runtime::engine::for_tab(&finalizer.owner.tab_id)
            });
            let mut startup_permit = startup_session
                .as_ref()
                .map(|session| session.gate.try_acquire());
            if let Some(epoch) = start_epoch {
                let still_ended = matches!(&startup_permit, Some(Ok(_)))
                    && startup_session.as_ref().is_some_and(|session| {
                        super::runtime_runs::unbound_start_epoch(
                            &svc,
                            session,
                            &finalizer.owner.tab_id,
                            finalizer.owner.binding_nonce.as_deref().unwrap_or_default(),
                        ) == Some(epoch)
                    });
                if !still_ended {
                    let _ = settled.send(Ok(false));
                    return Ok(());
                }
            }
            if !finalizer.role_settled.swap(true, Ordering::AcqRel)
                && finalizer.rejection.is_none()
                && cron.lock().run_is_active(&key.0, &key.1)
            {
                let job = cron.lock().get(&key.0).cloned();
                if let Some((job, policy)) = job.as_ref().and_then(|job| {
                    crate::service::role_policy::resolve(job.role.as_deref())
                        .map(|policy| (job, policy))
                }) {
                    let now = cron.lock().now_ms();
                    let settlement =
                        policy.settle_cron(&svc, job, &finalizer.owner.tab_id, &key.1, now);
                    let mut request = finalizer.request.lock();
                    match settlement {
                        Ok(done) => {
                            request.status = if done.success {
                                CronRunStatus::Success
                            } else {
                                CronRunStatus::Failed
                            };
                            request.summary =
                                recovery::bounded(&done.summary, recovery::MAX_SUMMARY);
                        }
                        Err(error) => {
                            request.status = CronRunStatus::Failed;
                            request.summary = recovery::bounded(
                                &format!("Run settlement failed: {error}"),
                                recovery::MAX_SUMMARY,
                            );
                        }
                    }
                }
            }
            let request = finalizer.request.lock().clone();
            loop {
                let terminal = !cron.lock().run_is_active(&key.0, &key.1);
                if finalizer.cancelled.load(Ordering::Acquire) && terminal {
                    drop(startup_permit.take());
                    cleanup(&svc, &cron, &finalizer.owner, TabCloseReason::Cancelled)?;
                    svc.run_watcher
                        .archive_cron_record(&cron, &key.0, &key.1, true)?;
                    let _ = settled.send(Ok(false));
                    break;
                }
                let phase = *finalizer.phase.lock();
                if phase == FinalPhase::Cleanup {
                    svc.run_watcher.checkpoint_cron(
                        &cron,
                        &finalizer.owner,
                        recovery::Phase::Cleanup,
                        (finalizer.raw_bytes, &finalizer.raw_output),
                        None,
                        None,
                    )?;
                    cleanup(&svc, &cron, &finalizer.owner, TabCloseReason::AgentRunEnded)?;
                    if let Some(cleaned) = cleaned.take() {
                        let _ = cleaned.send(Ok(()));
                    }
                    *finalizer.phase.lock() = FinalPhase::Delivery;
                    continue;
                }
                if phase == FinalPhase::Delivery {
                    let _delivery = finalizer.delivery_gate.lock();
                    if finalizer.cancelled.load(Ordering::Acquire) {
                        continue;
                    }
                    let plan = finalizer
                        .report
                        .lock()
                        .as_ref()
                        .map(|(plan, _)| plan.clone())
                        .unwrap_or_else(|| svc.cron_report_plan(&cron, &key.0));
                    let known = finalizer
                        .report
                        .lock()
                        .as_ref()
                        .map(|(_, label)| label.clone())
                        .filter(|label| !label.is_empty());
                    let label = if let Some(known) = known {
                        known
                    } else {
                        svc.run_watcher.checkpoint_cron(
                            &cron,
                            &finalizer.owner,
                            recovery::Phase::DeliveryIntent,
                            (finalizer.raw_bytes, &finalizer.raw_output),
                            Some(plan.clone()),
                            None,
                        )?;
                        let delivery = std::mem::replace(
                            &mut *finalizer.deliver.lock(),
                            DeliveryPort::Consumed,
                        );
                        let retained = cron.lock().retained_run_report(&key.0, &key.1);
                        let label = match (retained, delivery) {
                            (Some(retained), _) => retained,
                            (None, DeliveryPort::Injected(deliver)) => svc.deliver_cron_terminal_report(&plan, CronRunEnd { job_id: &key.0, run_id: &key.1, status: request.status.clone(), summary: &request.summary }, deliver),
                            (None, DeliveryPort::Production) => svc.deliver_cron_terminal_report(&plan, CronRunEnd { job_id: &key.0, run_id: &key.1, status: request.status.clone(), summary: &request.summary }, |tab, envelope, text| svc.deliver_cron_report(tab, envelope, text)),
                            (None, DeliveryPort::Consumed) => return Err("Cron delivery outcome is unknown; use cron.cancel to acknowledge without replay".into()),
                        };
                        *finalizer.report.lock() =
                            Some((plan.clone(), recovery::bounded(&label, 1024)));
                        label
                    };
                    if finalizer.cancelled.load(Ordering::Acquire)
                        && svc.run_watcher.cron_record(&key.0, &key.1).is_none()
                    {
                        continue;
                    }
                    svc.run_watcher.checkpoint_cron(
                        &cron,
                        &finalizer.owner,
                        recovery::Phase::DeliveryKnown,
                        (finalizer.raw_bytes, &finalizer.raw_output),
                        Some(plan),
                        Some(recovery::bounded(&label, 1024)),
                    )?;
                    *finalizer.phase.lock() = FinalPhase::Report;
                    continue;
                }
                let known = if phase == FinalPhase::Terminal {
                    *finalizer.known_finished.lock()
                } else {
                    None
                };
                let finished = if let Some(known) = known {
                    known
                } else {
                    if phase == FinalPhase::Terminal {
                        svc.run_watcher
                            .journal_cron_intent(&finalizer.owner, &request)?;
                        svc.run_watcher.checkpoint_cron(
                            &cron,
                            &finalizer.owner,
                            recovery::Phase::FinishPending,
                            (finalizer.raw_bytes, &finalizer.raw_output),
                            None,
                            None,
                        )?;
                    }
                    let mut attempt =
                        finalizer
                            .attempt
                            .lock()
                            .take()
                            .unwrap_or_else(|| match phase {
                                FinalPhase::Terminal => TerminalRunCommit::new(request.clone()),
                                FinalPhase::Report => TerminalRunCommit::report(
                                    request.clone(),
                                    finalizer
                                        .report
                                        .lock()
                                        .as_ref()
                                        .map(|(_, label)| label.clone())
                                        .unwrap_or_default(),
                                ),
                                FinalPhase::Retire => TerminalRunCommit::retire(request.clone()),
                                _ => unreachable!("effects have no persistence attempt"),
                            });
                    let result = if phase == FinalPhase::Terminal {
                        match finalizer.rejection.as_ref() {
                            Some(rejection) => {
                                crate::async_runtime::block_on(attempt.settle_guarded(
                                    &cron,
                                    || Ok(svc.mux.write()),
                                    |store, mux| rejection.admit(&svc, mux, store),
                                ))
                            }
                            None => crate::async_runtime::block_on(attempt.settle(&cron)),
                        }
                    } else {
                        crate::async_runtime::block_on(attempt.settle(&cron))
                    };
                    match result {
                        Ok(finished) => finished,
                        Err(error) => {
                            let unknown = matches!(
                                error,
                                crate::cron::store::CronCommitError::Unconfirmed(_)
                            );
                            *finalizer.attempt.lock() = Some(attempt);
                            if unknown {
                                finalizer.unknown.store(true, Ordering::Release);
                                svc.run_watcher.mark_cron_unknown(&finalizer.owner)?;
                                // The original completion ended without adjudicating the write.
                                // Bounded durable intent now owns that unknown; it is never resubmitted.
                                finalizer.attempt.lock().take();
                            }
                            return Err(error.to_string());
                        }
                    }
                };
                match phase {
                    FinalPhase::Terminal => {
                        *finalizer.known_finished.lock() = Some(finished);
                        drop(startup_permit.take());
                        if !finished.fresh {
                            let _ = settled.send(Ok(false));
                            if cron.lock().run_is_cancelled(&key.0, &key.1) {
                                cleanup(&svc, &cron, &finalizer.owner, TabCloseReason::Cancelled)?;
                                svc.run_watcher
                                    .archive_cron_record(&cron, &key.0, &key.1, true)?;
                                break;
                            }
                            svc.run_watcher.mark_cron_unknown(&finalizer.owner)?;
                            return Err("Another terminal owner won; original evidence retained for explicit cron.cancel adjudication".into());
                        }
                        svc.run_watcher.checkpoint_cron(
                            &cron,
                            &finalizer.owner,
                            recovery::Phase::Confirmed,
                            (finalizer.raw_bytes, &finalizer.raw_output),
                            None,
                            None,
                        )?;
                        let _ = settled.send(Ok(true));
                        if let Some(release) = response_release.take() {
                            let _ = release.recv();
                        }
                        if !finalizer.effects_started.swap(true, Ordering::AcqRel) {
                            let extent = finalizer.raw_bytes.map(|bytes| format!("complete input evidence: {bytes} bytes")).unwrap_or_else(|| "PARTIAL recent content projection; original bytecount unavailable".into());
                            let log_text = format!("--- SUMMARY ---\n{}\n\n--- BOUNDED OUTPUT EVIDENCE ({extent}; full transcript bytecount/digest unavailable) ---\n{}", request.summary, finalizer.raw_output);
                            if finalizer.plain_shell && request.status == CronRunStatus::Success {
                                let oneshot =
                                    cron.lock().get(&key.0).is_some_and(|job| job.oneshot);
                                log::info!("Cron run completed: job={}, run={}, oneshot={}, summary_len={}", key.0, key.1, oneshot, log_text.len());
                            } else {
                                log_finished(&key.0, &key.1, &request.status, finished);
                            }
                            let path = cron.lock().log_file(&key.0, &key.1)?;
                            if let Err(error) = CronStore::create_log(&path, &log_text) {
                                log::error!("Failed to save cron log: {error}");
                            }
                            let plan = svc.cron_report_plan(&cron, &key.0);
                            crate::cron::notify::notify_on_consecutive_failure(&svc, &cron, &key.0);
                            *finalizer.report.lock() = Some((plan, String::new()));
                        }
                        *finalizer.phase.lock() = FinalPhase::Cleanup;
                    }
                    FinalPhase::Report => {
                        svc.run_watcher.checkpoint_cron(
                            &cron,
                            &finalizer.owner,
                            recovery::Phase::Report,
                            (finalizer.raw_bytes, &finalizer.raw_output),
                            None,
                            None,
                        )?;
                        if finalizer
                            .report
                            .lock()
                            .as_ref()
                            .is_some_and(|(plan, _)| plan.retire_oneshot)
                        {
                            *finalizer.phase.lock() = FinalPhase::Retire;
                        } else {
                            svc.run_watcher
                                .archive_cron_record(&cron, &key.0, &key.1, false)?;
                            break;
                        }
                    }
                    FinalPhase::Retire => {
                        svc.run_watcher
                            .archive_cron_record(&cron, &key.0, &key.1, false)?;
                        svc.emit_cron_changed();
                        break;
                    }
                    _ => unreachable!("effects settle separately"),
                }
            }
            Ok(())
        })();
        let result = if finalizer.cancelled.load(Ordering::Acquire)
            && svc.run_watcher.cron_record(&key.0, &key.1).is_none()
        {
            Ok(())
        } else {
            result
        };
        if let Err(error) = result {
            let detail = recovery::bounded(
                &format!(
                    "Cron terminal owner retained: job={} run={}: {error}",
                    key.0, key.1
                ),
                2048,
            );
            *finalizer.failure.lock() = Some(detail.clone());
            log::error!("{detail}");
            svc.log_system_notice(
                "Cron terminal continuation requires recovery".into(),
                Some(detail.clone()),
                Some("error"),
            );
            if let Some(cleaned) = cleaned.take() {
                let _ = cleaned.send(Err(detail.clone()));
            }
            let _ = settled.send(Err(detail));
            return;
        }
        if let Some(cleaned) = cleaned.take() {
            let _ = cleaned.send(Ok(()));
        }
        let mut owners = svc.run_watcher.cron_finalizers.lock();
        if owners
            .get(&key)
            .is_some_and(|current| Arc::ptr_eq(current, &finalizer))
        {
            owners.remove(&key);
        }
        drop(owners);
        if let Some(wake) = svc.try_ext::<crate::CronWake>() {
            wake.notify_all();
        }
        svc.ui.emit("cron-run-finished", serde_json::Value::Null);
    });
    Some(CronEndHandle { terminal, thread })
}

fn cleanup(
    svc: &ServiceContext,
    cron: &crate::CronState,
    owner: &CronRunAuthority,
    reason: TabCloseReason,
) -> Result<(), String> {
    svc.run_watcher.retain_cron_cleanup(owner, None);
    svc.run_watcher.retire_cron(owner);
    {
        let mut store = cron.lock();
        if store
            .get_tab_owner(owner.tab_id())
            .is_some_and(|(job, run)| job == owner.job_id() && run == owner.run_id())
        {
            store.unregister_tab(owner.tab_id());
        }
    }
    svc.settle_owned_cron_cleanup(owner, reason)
}

impl super::RunWatcherRegistry {
    pub(crate) fn retain_cancellation_finalizers(&self, job: &str) {
        let owners: Vec<_> = self
            .cron_finalizers
            .lock()
            .values()
            .filter(|entry| entry.owner.job_id == job)
            .map(|entry| entry.owner.clone())
            .collect();
        for owner in owners {
            self.retain_cron_cleanup(&owner, None);
        }
    }

    pub(crate) fn cancel_cron_finalizer(&self, owner: &CronRunAuthority) {
        let finalizer = self
            .cron_finalizers
            .lock()
            .get(&(owner.job_id().into(), owner.run_id().into()))
            .filter(|finalizer| {
                finalizer.owner.tab_id == owner.tab_id
                    && finalizer.owner.binding_nonce == owner.binding_nonce
                    && finalizer
                        .owner
                        .pty_id
                        .as_ref()
                        .is_none_or(|pty| owner.pty_id.as_ref() == Some(pty))
            })
            .cloned();
        if let Some(finalizer) = finalizer {
            let _delivery = finalizer.delivery_gate.lock();
            finalizer.cancelled.store(true, Ordering::Release);
        }
    }

    pub(crate) fn cron_job_finalizing(&self, job: &str) -> bool {
        let cancelling = self.cron_cancelling.lock().contains(job);
        let allocating = self
            .cron_allocations
            .lock()
            .iter()
            .any(|(owned, _)| owned == job);
        let finalizing = self
            .cron_finalizers
            .lock()
            .keys()
            .any(|(owned, _)| owned == job);
        let cleaning = self
            .cron_cleanup
            .lock()
            .keys()
            .any(|(owned, _)| owned == job);
        cancelling || allocating || finalizing || cleaning || self.retained_cron_jobs(job)
    }

    pub(crate) fn resume_allocation_finalizer(
        &self,
        svc: &ServiceContext,
        owner: &CronRunAuthority,
    ) {
        let retained = self
            .cron_finalizers
            .lock()
            .get(&(owner.job_id().into(), owner.run_id().into()))
            .cloned();
        if let Some(retained) = retained {
            if retained.owner.tab_id != owner.tab_id
                || retained.owner.binding_nonce != owner.binding_nonce
            {
                return;
            }
            retained
                .resume_after_allocation
                .store(true, Ordering::Release);
            if !retained.running.load(Ordering::Acquire) {
                retained
                    .resume_after_allocation
                    .store(false, Ordering::Release);
                restart_finalizer(svc, &retained);
            }
        } else {
            let _ = finish(
                svc,
                EndedCronRun {
                    tab_id: owner.tab_id(),
                    job_id: owner.job_id(),
                    run_id: owner.run_id(),
                    success: false,
                    summary: String::new(),
                    raw_output: "",
                    session_id: None,
                    cwd: None,
                    binding_nonce: owner.nonce(),
                    pty_id: owner.pty_id(),
                    failed_start_epoch: None,
                },
            );
        }
    }

    pub(crate) fn finish_cron_after_response(
        &self,
        svc: &ServiceContext,
        owner: &CronRunAuthority,
        summary: String,
        raw: &str,
        session: Option<&str>,
        cwd: Option<&str>,
    ) -> Result<Option<ResponseRelease>, String> {
        finish_after_response(svc, owner, summary, raw, session, cwd)
    }

    pub(crate) fn resume_cron_finalizer(&self, svc: &ServiceContext, tab: &str, run: &str) -> bool {
        let nonce = svc
            .mux
            .read()
            .get_tab(tab)
            .and_then(|tab| tab.session_binding_nonce.clone());
        self.resume_cron_finalizer_for_run(svc, run, Some(tab), nonce.as_deref(), false)
    }

    pub(crate) fn resume_cron_finalizer_for_run(
        &self,
        svc: &ServiceContext,
        run: &str,
        caller: Option<&str>,
        nonce: Option<&str>,
        human: bool,
    ) -> bool {
        let retained = self
            .cron_finalizers
            .lock()
            .values()
            .find(|owner| {
                owner.owner.run_id == run
                    && owner.failure.lock().is_some()
                    && (human
                        || caller == Some(owner.owner.tab_id.as_str())
                            && nonce == owner.owner.binding_nonce.as_deref())
            })
            .cloned();
        let Some(retained) = retained else {
            return false;
        };
        let request = retained.request.lock().clone();
        let _ = finish(
            svc,
            EndedCronRun {
                tab_id: &retained.owner.tab_id,
                job_id: &request.job_id,
                run_id: run,
                success: request.status == CronRunStatus::Success,
                summary: request.summary,
                raw_output: &retained.raw_output,
                session_id: request.session_id.as_deref(),
                cwd: request.cwd.as_deref(),
                binding_nonce: retained.owner.binding_nonce.as_deref(),
                pty_id: retained.owner.pty_id.as_deref(),
                failed_start_epoch: retained.failed_start_epoch,
            },
        );
        true
    }

    pub(crate) fn forget_adjudicated_finalizer(&self, owner: &CronRunAuthority) {
        let key = (owner.job_id().into(), owner.run_id().into());
        let mut owners = self.cron_finalizers.lock();
        if owners
            .get(&key)
            .is_some_and(|entry| entry.owner == *owner && !entry.running.load(Ordering::Acquire))
        {
            owners.remove(&key);
        }
    }

    pub(crate) fn forget_cancelled_cron_finalizer(&self, owner: &CronRunAuthority) {
        let key = (owner.job_id.clone(), owner.run_id.clone());
        let mut owners = self.cron_finalizers.lock();
        if owners
            .get(&key)
            .is_some_and(|entry| entry.owner == *owner && !entry.running.load(Ordering::Acquire))
        {
            owners.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests;
