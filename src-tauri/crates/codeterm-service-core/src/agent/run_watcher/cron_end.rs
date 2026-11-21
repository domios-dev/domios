use std::sync::{atomic::{AtomicBool, Ordering}, Arc};
use std::thread::JoinHandle;
use parking_lot::Mutex;

use crate::cron::store::{CronRunStatus, CronStore};
use crate::cron::store::storage::{log_finished, TerminalRun, TerminalRunCommit};
use crate::multiplexer::types::TabCloseReason;
use crate::service::cron::CronRunEnd;
use crate::service::messenger::MessageEnvelope;
use crate::service::ServiceContext;
use super::runtime_runs::CronRunAuthority;

type Deliver = Box<dyn FnOnce(&str, MessageEnvelope, &str) -> Result<(), String> + Send>;
enum DeliveryPort { Production, Injected(Deliver), Consumed }

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
    attempt: Mutex<Option<TerminalRunCommit>>,
    deliver: Mutex<DeliveryPort>,
    running: AtomicBool,
    effects_started: AtomicBool,
    role_settled: AtomicBool,
    failure: Mutex<Option<String>>,
    failed_start_epoch: Option<u64>,
    phase: Mutex<FinalPhase>,
    report: Mutex<Option<(crate::service::cron::CronReportPlan, String)>>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum FinalPhase { Terminal, Cleanup, Delivery, Report, Retire }

pub(crate) struct CronEndHandle {
    terminal: std::sync::mpsc::Receiver<Result<bool, String>>,
    thread: JoinHandle<()>,
}

impl CronEndHandle {
    pub(super) fn wait_for_terminal(self) -> Result<bool, String> {
        let result = self.terminal.recv().map_err(|_| "Cron terminal owner ended without an outcome".to_string())?;
        drop(self.thread);
        result
    }

    #[cfg(test)]
    pub(super) fn join(self) -> std::thread::Result<()> { self.thread.join() }
}

pub(super) fn finish(svc: &ServiceContext, run: EndedCronRun<'_>) -> Option<CronEndHandle> {
    start(svc, run, None)
}

#[cfg(test)]
fn finish_with<D>(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: D) -> Option<JoinHandle<()>>
where D: FnOnce(&str, MessageEnvelope, &str) -> Result<(), String> + Send + 'static,
{
    start(svc, run, Some(Box::new(deliver))).map(|handle| handle.thread)
}

struct Running<'a>(&'a AtomicBool);
impl Drop for Running<'_> {
    fn drop(&mut self) { self.0.store(false, Ordering::Release); }
}

fn start(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: Option<Deliver>) -> Option<CronEndHandle> {
    start_gated(svc, run, deliver, None)
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
        self.cleaned.recv().map_err(|_| "Cron cleanup owner ended without a receipt".to_string())?
    }
}

pub(crate) fn finish_after_response(svc: &ServiceContext, owner: &CronRunAuthority, summary: String, raw: &str, session: Option<&str>, cwd: Option<&str>) -> Result<Option<ResponseRelease>, String> {
    let (release, wait) = std::sync::mpsc::channel();
    let (cleaned, receipt) = std::sync::mpsc::channel();
    let handle = start_gated(svc, EndedCronRun {
        tab_id: owner.tab_id(), job_id: owner.job_id(), run_id: owner.run_id(), success: true, summary,
        raw_output: raw, session_id: session, cwd, binding_nonce: owner.nonce(), pty_id: owner.pty_id(), failed_start_epoch: None,
    }, None, Some(ResponseFence { release: wait, cleaned }));
    let Some(handle) = handle else { return Ok(None) };
    if !handle.wait_for_terminal()? { return Ok(None); }
    Ok(Some(ResponseRelease { release, cleaned: receipt }))
}

fn start_gated(svc: &ServiceContext, run: EndedCronRun<'_>, deliver: Option<Deliver>, response: Option<ResponseFence>) -> Option<CronEndHandle> {
    let cron = svc.try_ext::<crate::CronState>()?.clone();
    let key = (run.job_id.to_string(), run.run_id.to_string());
    let own = CronRunAuthority {
        tab_id: run.tab_id.into(), job_id: run.job_id.into(), run_id: run.run_id.into(),
        binding_nonce: run.binding_nonce.map(str::to_string), pty_id: run.pty_id.map(str::to_string),
    };
    let finalizer = {
        let mut owners = svc.run_watcher.cron_finalizers.lock();
        let entry = owners.entry(key.clone()).or_insert_with(|| Arc::new(Finalizer {
            owner: own.clone(),
            request: Mutex::new(TerminalRun {
                job_id: run.job_id.into(), run_id: run.run_id.into(),
                status: if run.success { CronRunStatus::Success } else { CronRunStatus::Failed },
                summary: run.summary, session_id: run.session_id.map(str::to_string), cwd: run.cwd.map(str::to_string),
                tab_id: Some(run.tab_id.into()),
            }),
            raw_output: run.raw_output.into(), attempt: Mutex::new(None), deliver: Mutex::new(deliver.map(DeliveryPort::Injected).unwrap_or(DeliveryPort::Production)),
            running: AtomicBool::new(false), effects_started: AtomicBool::new(false), role_settled: AtomicBool::new(false),
            failure: Mutex::new(None),
            failed_start_epoch: run.failed_start_epoch,
            phase: Mutex::new(FinalPhase::Terminal), report: Mutex::new(None),
        }));
        if entry.owner != own || entry.running.swap(true, Ordering::AcqRel) { return None; }
        entry.clone()
    };
    let svc = svc.clone();
    let (settled, terminal) = std::sync::mpsc::channel();
    let thread = codeterm_util::named_thread::spawn("cron-run-end", move || {
        let _running = Running(&finalizer.running);
        let start_epoch = if *finalizer.phase.lock() == FinalPhase::Terminal { finalizer.failed_start_epoch } else { None };
        let startup_session = start_epoch.and_then(|_| crate::service::agent_runtime::engine::for_tab(&finalizer.owner.tab_id));
        let mut startup_permit = startup_session.as_ref().map(|session| session.gate.try_acquire());
        if let Some(epoch) = start_epoch {
            let still_ended = matches!(&startup_permit, Some(Ok(_))) && startup_session.as_ref().is_some_and(|session| {
                super::runtime_runs::unbound_start_epoch(&svc, session, &finalizer.owner.tab_id, finalizer.owner.binding_nonce.as_deref().unwrap_or_default()) == Some(epoch)
            });
            if !still_ended {
                svc.run_watcher.cron_finalizers.lock().remove(&key);
                let _ = settled.send(Ok(false));
                return;
            }
        }
        if !finalizer.role_settled.swap(true, Ordering::AcqRel) && cron.lock().run_is_active(&key.0, &key.1) {
            let job = cron.lock().get(&key.0).cloned();
            if let Some((job, policy)) = job.as_ref().and_then(|job| crate::service::role_policy::resolve(job.role.as_deref()).map(|policy| (job, policy))) {
                let now = cron.lock().now_ms();
                let settlement = policy.settle_cron(&svc, job, &finalizer.owner.tab_id, &key.1, now);
                let mut request = finalizer.request.lock();
                match settlement {
                    Ok(done) => {
                        request.status = if done.success { CronRunStatus::Success } else { CronRunStatus::Failed };
                        request.summary = done.summary;
                    }
                    Err(error) => {
                        request.status = CronRunStatus::Failed;
                        request.summary = format!("Run settlement failed: {error}");
                    }
                }
            }
        }
        let request = finalizer.request.lock().clone();
        let (mut response_release, mut cleaned) = match response {
            Some(fence) => (Some(fence.release), Some(fence.cleaned)),
            None => (None, None),
        };
        loop {
            let phase = *finalizer.phase.lock();
            if phase == FinalPhase::Cleanup {
                if let Err(error) = svc.settle_owned_cron_cleanup(&finalizer.owner, TabCloseReason::AgentRunEnded) {
                    let detail = format!("Cron terminal cleanup requires recovery: job={} run={}: {error}", key.0, key.1);
                    log::error!("{detail}");
                    svc.log_system_notice("Cron terminal cleanup failed".into(), Some(detail.clone()), Some("error"));
                    *finalizer.failure.lock() = Some(detail.clone());
                    if let Some(cleaned) = cleaned.take() { let _ = cleaned.send(Err(detail.clone())); }
                    let _ = settled.send(Err(detail));
                    return;
                }
                if let Some(cleaned) = cleaned.take() { let _ = cleaned.send(Ok(())); }
                *finalizer.phase.lock() = FinalPhase::Delivery;
                continue;
            }
            if phase == FinalPhase::Delivery {
                let plan = finalizer.report.lock().as_ref().map(|(plan, _)| plan.clone()).unwrap_or_else(|| svc.cron_report_plan(&cron, &key.0));
                    let delivery = std::mem::replace(&mut *finalizer.deliver.lock(), DeliveryPort::Consumed);
                    let retained = cron.lock().retained_run_report(&key.0, &key.1);
                    let label = match (retained, delivery) {
                        (Some(retained), _) => retained,
                        (None, DeliveryPort::Injected(deliver)) => svc.deliver_cron_terminal_report(&plan, CronRunEnd {
                            job_id: &key.0, run_id: &key.1, status: request.status.clone(), summary: &request.summary,
                        }, deliver),
                        (None, DeliveryPort::Production) => svc.deliver_cron_terminal_report(&plan, CronRunEnd {
                            job_id: &key.0, run_id: &key.1, status: request.status.clone(), summary: &request.summary,
                        }, |tab, envelope, text| svc.deliver_cron_report(tab, envelope, text)),
                        (None, DeliveryPort::Consumed) => "delivered_unconfirmed".into(),
                    };
                    *finalizer.report.lock() = Some((plan, label));
                *finalizer.phase.lock() = FinalPhase::Report;
                continue;
            }
            let mut attempt = finalizer.attempt.lock().take().unwrap_or_else(|| match phase {
                FinalPhase::Terminal => TerminalRunCommit::new(request.clone()),
                FinalPhase::Cleanup | FinalPhase::Delivery => unreachable!("effects do not write cron state"),
                FinalPhase::Report => TerminalRunCommit::report(request.clone(), finalizer.report.lock().as_ref().map(|(_, label)| label.clone()).unwrap_or_else(|| "delivered_unconfirmed".into())),
                FinalPhase::Retire => TerminalRunCommit::retire(request.clone()),
            });
            let finished = match crate::async_runtime::block_on(attempt.settle(&cron)) {
                Ok(finished) => finished,
                Err(error) => {
                    *finalizer.attempt.lock() = Some(attempt);
                    let detail = format!("Cron terminal persistence requires recovery: job={} run={}: {error}", key.0, key.1);
                    let mut prior = finalizer.failure.lock();
                    if prior.as_ref() != Some(&detail) {
                        log::error!("{detail}");
                        svc.log_system_notice("Cron terminal persistence failed".into(), Some(detail.clone()), Some("error"));
                        *prior = Some(detail.clone());
                    }
                    let _ = settled.send(Err(detail));
                    return;
                }
            };
            match phase {
                FinalPhase::Terminal => {
                    drop(startup_permit.take());
                    let _ = settled.send(Ok(finished.fresh));
                    if let Some(release) = response_release.take() { let _ = release.recv(); }
                    svc.run_watcher.retire_cron(&finalizer.owner);
                    {
                        let mut store = cron.lock();
                        if store.get_tab_owner(&finalizer.owner.tab_id).is_some_and(|owner| owner == &key) {
                            store.unregister_tab(&finalizer.owner.tab_id);
                        }
                    }
                    if !finished.fresh {
                        svc.run_watcher.release_cron_binding(&finalizer.owner);
                        break;
                    }
                    if !finalizer.effects_started.swap(true, Ordering::AcqRel) {
                        log_finished(&key.0, &key.1, &request.status, finished);
                        let log_text = format!("--- SUMMARY ---\n{}\n\n--- RAW OUTPUT ---\n{}", request.summary, finalizer.raw_output);
                        let path = cron.lock().log_file(&key.0, &key.1);
                        if let Err(error) = path.and_then(|path| CronStore::create_log(&path, &log_text).map(|_| ())) {
                            log::error!("Failed to save cron log: {error}");
                        }
                        let plan = svc.cron_report_plan(&cron, &key.0);
                        crate::cron::notify::notify_on_consecutive_failure(&svc, &cron, &key.0);
                        *finalizer.report.lock() = Some((plan, String::new()));
                    }
                    *finalizer.phase.lock() = FinalPhase::Cleanup;
                }
                FinalPhase::Cleanup | FinalPhase::Delivery => unreachable!("effects settle before persistence"),
                FinalPhase::Report => {
                    if !finalizer.report.lock().as_ref().is_some_and(|(plan, _)| plan.retire_oneshot) { break; }
                    *finalizer.phase.lock() = FinalPhase::Retire;
                }
                FinalPhase::Retire => {
                    if let Some(wake) = svc.try_ext::<crate::CronWake>() { wake.notify_all(); }
                    svc.emit_cron_changed();
                    break;
                }
            }
        }
        if let Some(cleaned) = cleaned.take() { let _ = cleaned.send(Ok(())); }
        let mut owners = svc.run_watcher.cron_finalizers.lock();
        if owners.get(&key).is_some_and(|current| Arc::ptr_eq(current, &finalizer)) { owners.remove(&key); }
        drop(owners);
        if let Some(wake) = svc.try_ext::<crate::CronWake>() { wake.notify_all(); }
        svc.ui.emit("cron-run-finished", serde_json::to_value(()).unwrap_or_default());
    });
    Some(CronEndHandle { terminal, thread })
}

impl super::RunWatcherRegistry {
    pub(crate) fn cron_job_finalizing(&self, job: &str) -> bool {
        self.cron_finalizers.lock().keys().any(|(owned, _)| owned == job)
            || self.cron_cleanup.lock().keys().any(|(owned, _)| owned == job)
    }

    pub(crate) fn finish_cron_after_response(&self, svc: &ServiceContext, owner: &CronRunAuthority, summary: String, raw: &str, session: Option<&str>, cwd: Option<&str>) -> Result<Option<ResponseRelease>, String> {
        finish_after_response(svc, owner, summary, raw, session, cwd)
    }

    pub(crate) fn resume_cron_finalizer(&self, svc: &ServiceContext, tab: &str, run: &str) -> bool {
        let nonce = svc.mux.read().get_tab(tab).and_then(|tab| tab.session_binding_nonce.clone());
        self.resume_cron_finalizer_for_run(svc, run, Some(tab), nonce.as_deref(), false)
    }

    pub(crate) fn resume_cron_finalizer_for_run(&self, svc: &ServiceContext, run: &str, caller: Option<&str>, nonce: Option<&str>, human: bool) -> bool {
        let retained = self.cron_finalizers.lock().values().find(|owner| {
            owner.owner.run_id == run && owner.failure.lock().is_some()
                && (human || caller == Some(owner.owner.tab_id.as_str()) && nonce == owner.owner.binding_nonce.as_deref())
        }).cloned();
        let Some(retained) = retained else { return false };
        let request = retained.request.lock().clone();
        let _ = finish(svc, EndedCronRun {
            tab_id: &retained.owner.tab_id, job_id: &request.job_id, run_id: run,
            success: request.status == CronRunStatus::Success, summary: request.summary,
            raw_output: &retained.raw_output, session_id: request.session_id.as_deref(), cwd: request.cwd.as_deref(),
            binding_nonce: retained.owner.binding_nonce.as_deref(), pty_id: retained.owner.pty_id.as_deref(),
            failed_start_epoch: retained.failed_start_epoch,
        });
        true
    }

    pub(crate) fn forget_cancelled_cron_finalizer(&self, owner: &CronRunAuthority) {
        let key = (owner.job_id.clone(), owner.run_id.clone());
        let mut owners = self.cron_finalizers.lock();
        if owners.get(&key).is_some_and(|entry| entry.owner == *owner && !entry.running.load(Ordering::Acquire)) {
            owners.remove(&key);
        }
    }
}

#[cfg(test)]
mod tests;
