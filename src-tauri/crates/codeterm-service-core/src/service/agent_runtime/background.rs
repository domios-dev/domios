use super::{
    engine::{self, Session},
    model::RuntimeFailure,
    process::Process,
    process_boundary::Member,
};
use crate::service::ServiceContext;

mod os_watch;
mod roster;
use codeterm_plugin::agent_runtime::*;
pub(super) use roster::{changed_by, preserve_projection, watch};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, sync::Arc, time::Duration};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Interrupt,
    Shutdown,
    Failure,
}

#[async_trait::async_trait]
trait CleanupIo {
    async fn run(&mut self, operation: RuntimeOperation) -> Result<RuntimeResult, RuntimeFailure>;
    async fn verify(
        &mut self,
        roster: Vec<RuntimeBackgroundProcess>,
    ) -> Result<Vec<u32>, RuntimeFailure>;
    fn notice(&mut self, unknown: &[u32]);
    async fn finish(&mut self) -> Result<(), RuntimeFailure>;
    fn ended(&self) -> bool {
        false
    }
}

pub(super) fn supported(session: &Session) -> bool {
    session
        .adapter
        .lock()
        .as_ref()
        .and_then(|adapter| adapter.agent_runtime())
        .is_some_and(|cap| cap.background_processes)
}

struct RuntimeIo<'a> {
    svc: &'a ServiceContext,
    session: &'a Arc<Session>,
    process: &'a Arc<Process>,
    captured: Vec<Member>,
    failure_cleanup: bool,
}
#[async_trait::async_trait]
impl CleanupIo for RuntimeIo<'_> {
    async fn run(&mut self, operation: RuntimeOperation) -> Result<RuntimeResult, RuntimeFailure> {
        engine::run(
            self.svc,
            &self.session.provider,
            self.session,
            self.process,
            operation,
        )
        .await
    }
    async fn verify(
        &mut self,
        roster: Vec<RuntimeBackgroundProcess>,
    ) -> Result<Vec<u32>, RuntimeFailure> {
        self.process
            .verify_interrupt(std::mem::take(&mut self.captured), roster)
            .await
    }
    fn notice(&mut self, unknown: &[u32]) {
        self.svc.log_system_notice(
            "Runtime processes retained".into(),
            Some(format!("Tab {} retained unattributed processes {unknown:?}; only roster-matched commands are stopped on interrupt.", self.session.context.lock().tab_id)),
            Some("info"),
        );
    }
    async fn finish(&mut self) -> Result<(), RuntimeFailure> {
        if self.failure_cleanup {
            self.process.finish_after_failure().await?;
        } else {
            self.process.finish().await?;
        }
        Ok(())
    }
    fn ended(&self) -> bool {
        self.process.terminated()
    }
}

pub(super) async fn cleanup(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    mode: Mode,
    captured: Vec<Member>,
    rpc_allowed: bool,
) -> Result<(), RuntimeFailure> {
    let context = session.context.lock().clone();
    let enabled = supported(session) && rpc_allowed;
    let mut io = RuntimeIo {
        svc,
        session,
        process,
        captured,
        failure_cleanup: mode == Mode::Failure,
    };
    let result = coordinate(&mut io, &context, mode, enabled).await;
    if enabled && mode == Mode::Interrupt {
        process.background_changed.notify_one();
    }
    result
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackgroundProcessAuthority {
    pub root_thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StopBackgroundProcessRequest {
    pub root_thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
    pub thread_id: String,
    pub process_id: String,
}

impl StopBackgroundProcessRequest {
    fn authority(&self) -> BackgroundProcessAuthority {
        BackgroundProcessAuthority {
            root_thread_id: self.root_thread_id.clone(),
            binding_nonce: self.binding_nonce.clone(),
            epoch: self.epoch,
        }
    }
}

pub use codeterm_plugin::api_contracts::BackgroundProcessEntry;

pub use codeterm_plugin::api_contracts::BackgroundProcessSnapshot;

#[derive(Debug, Serialize)]
#[serde(tag = "status", rename_all = "camelCase")]
pub enum BackgroundStopResult {
    Stopped { snapshot: BackgroundProcessSnapshot },
    Degraded { reason: BackgroundStopDegradation },
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum BackgroundStopDegradation {
    AttributionUnavailable,
    ExitUnconfirmed,
}

fn session_for_authority(
    svc: &ServiceContext,
    tab: &str,
    authority: &BackgroundProcessAuthority,
) -> Result<(Arc<Session>, RuntimeContext, Arc<Process>), RuntimeFailure> {
    let session = engine::for_tab(tab).ok_or(RuntimeFailure::Superseded)?;
    engine::validate_binding(svc, &session)?;
    let context = session.context.lock().clone();
    if context.epoch != authority.epoch
        || context.binding_nonce != authority.binding_nonce
        || context.thread_id.as_deref() != Some(authority.root_thread_id.as_str())
    {
        return Err(RuntimeFailure::Superseded);
    }
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::PipeEof)?;
    Ok((session, context, process))
}

fn validate_current_session(
    svc: &ServiceContext,
    tab: &str,
    authority: &BackgroundProcessAuthority,
    session: &Arc<Session>,
) -> Result<(), RuntimeFailure> {
    if !engine::for_tab(tab).is_some_and(|current| Arc::ptr_eq(&current, session)) {
        return Err(RuntimeFailure::Superseded);
    }
    engine::validate_binding(svc, session)?;
    let context = session.context.lock().clone();
    if context.epoch != authority.epoch
        || context.binding_nonce != authority.binding_nonce
        || context.thread_id.as_deref() != Some(authority.root_thread_id.as_str())
    {
        return Err(RuntimeFailure::Superseded);
    }
    Ok(())
}

async fn list_owned(
    io: &mut impl CleanupIo,
    context: &RuntimeContext,
) -> Result<Vec<BackgroundProcessEntry>, RuntimeFailure> {
    let root = context
        .thread_id
        .as_deref()
        .ok_or(RuntimeFailure::Superseded)?;
    let mut threads = vec![root.to_owned()];
    threads.extend(
        context
            .thread_parents
            .iter()
            .filter(|parent| owns_runtime_thread(context, &parent.thread_id))
            .map(|parent| parent.thread_id.clone()),
    );
    threads.sort();
    threads.dedup();
    let mut processes = Vec::new();
    for thread_id in threads {
        let roster = list(io, &thread_id).await?;
        if processes.len() + roster.len() > MAX_BACKGROUND_PROCESSES {
            return Err(refused(
                "Owned background process roster exceeded its bound",
            ));
        }
        processes.extend(roster.into_iter().map(|process| BackgroundProcessEntry {
            thread_id: thread_id.clone(),
            process,
        }));
    }
    Ok(processes)
}

fn snapshot(
    authority: &BackgroundProcessAuthority,
    processes: Vec<BackgroundProcessEntry>,
) -> BackgroundProcessSnapshot {
    BackgroundProcessSnapshot {
        root_thread_id: authority.root_thread_id.clone(),
        binding_nonce: authority.binding_nonce.clone(),
        epoch: authority.epoch,
        processes,
    }
}

pub async fn list_for_tab(
    svc: &ServiceContext,
    tab: &str,
    authority: BackgroundProcessAuthority,
) -> Result<BackgroundProcessSnapshot, String> {
    list_for_tab_inner(svc, tab, authority)
        .await
        .map_err(|reason| reason.to_string())
}

async fn list_for_tab_inner(
    svc: &ServiceContext,
    tab: &str,
    authority: BackgroundProcessAuthority,
) -> Result<BackgroundProcessSnapshot, RuntimeFailure> {
    let (session, context, process) = session_for_authority(svc, tab, &authority)?;
    if !supported(&session) {
        return os_watch::list(svc, &session, &process, &context, &authority).await;
    }
    roster::refresh(svc, &session, &process, &context, &authority).await
}

async fn read_owned(
    svc: &ServiceContext,
    session: &Arc<Session>,
    process: &Arc<Process>,
    context: &RuntimeContext,
) -> Result<Vec<BackgroundProcessEntry>, RuntimeFailure> {
    let mut io = RuntimeIo {
        svc,
        session,
        process,
        captured: Vec::new(),
        failure_cleanup: false,
    };
    list_owned(&mut io, context).await
}

pub async fn stop_for_tab(
    svc: &ServiceContext,
    tab: &str,
    request: StopBackgroundProcessRequest,
) -> Result<BackgroundStopResult, String> {
    let svc = svc.clone();
    let tab = tab.to_owned();
    tokio::spawn(async move { stop_for_tab_inner(&svc, &tab, request).await })
        .await
        .map_err(|reason| reason.to_string())?
        .map_err(|reason| reason.to_string())
}

async fn stop_for_tab_inner(
    svc: &ServiceContext,
    tab: &str,
    request: StopBackgroundProcessRequest,
) -> Result<BackgroundStopResult, RuntimeFailure> {
    if request.process_id.is_empty()
        || request.process_id.len() > 4096
        || request.process_id.contains('\0')
    {
        return Err(refused("Invalid background process id"));
    }
    let authority = request.authority();
    let (session, context, process) = session_for_authority(svc, tab, &authority)?;
    if !supported(&session) {
        return os_watch::stop(
            svc,
            tab,
            &session,
            &process,
            &context,
            &authority,
            &request.thread_id,
            &request.process_id,
        )
        .await;
    }
    let _operation = process.background_operation.lock().await;
    validate_current_session(svc, tab, &authority, &session)?;
    if !owns_runtime_thread(&context, &request.thread_id) {
        return Err(RuntimeFailure::Superseded);
    }
    let mut io = RuntimeIo {
        svc,
        session: &session,
        process: &process,
        captured: Vec::new(),
        failure_cleanup: false,
    };
    let before = list_owned(&mut io, &context).await?;
    validate_current_session(svc, tab, &authority, &session)?;
    let target_position = before.iter().position(|entry| {
        entry.thread_id == request.thread_id && entry.process.process_id == request.process_id
    });
    #[cfg(windows)]
    let tree = match target_position {
        Some(position) => {
            let mut before = before;
            let target = before.swap_remove(position).process;
            let others = before.into_iter().map(|entry| entry.process).collect();
            match process.capture_background(target, others).await {
                Ok(tree) => tree,
                Err(reason) => {
                    log::debug!("[background-work] tab={tab} capture: {reason}");
                    None
                }
            }
        }
        None => None,
    };
    validate_current_session(svc, tab, &authority, &session)?;
    if target_position.is_some() {
        let result = io
            .run(RuntimeOperation::BackgroundProcesses {
                thread_id: request.thread_id.clone(),
                action: RuntimeBackgroundProcessAction::Terminate {
                    process_id: request.process_id.clone(),
                },
            })
            .await?;
        if !matches!(result, RuntimeResult::BackgroundProcessTermination { thread_id, process_id, .. }
            if thread_id == request.thread_id && process_id == request.process_id)
        {
            return Err(refused(
                "Background termination returned the wrong identity",
            ));
        }
    }
    let remaining = list(&mut io, &request.thread_id).await?;
    validate_current_session(svc, tab, &authority, &session)?;
    if remaining
        .iter()
        .any(|entry| entry.process_id == request.process_id)
    {
        return Err(refused("Background process remains after termination"));
    }
    #[cfg(windows)]
    let cleanup = match tree {
        Some(tree) => process.reap_background(tree).await.map_err(|reason| {
            log::debug!("[background-work] tab={tab} reap: {reason}");
            BackgroundStopDegradation::ExitUnconfirmed
        }),
        None => Err(BackgroundStopDegradation::AttributionUnavailable),
    };
    #[cfg(not(windows))]
    let cleanup = Err(BackgroundStopDegradation::AttributionUnavailable);
    let processes = list_owned(&mut io, &context).await?;
    validate_current_session(svc, tab, &authority, &session)?;
    let snapshot = snapshot(&authority, processes);
    roster::publish(svc, &context, Some(&snapshot));
    Ok(match cleanup {
        Ok(()) => BackgroundStopResult::Stopped { snapshot },
        Err(reason) => BackgroundStopResult::Degraded { reason },
    })
}

async fn coordinate(
    io: &mut impl CleanupIo,
    context: &RuntimeContext,
    mode: Mode,
    enabled: bool,
) -> Result<(), RuntimeFailure> {
    let mut roster = Vec::new();
    let protocol = if enabled {
        tokio::time::timeout(
            Duration::from_secs(15),
            drain(io, context, mode, &mut roster),
        )
        .await
        .map_err(|_| RuntimeFailure::Deadline)
        .and_then(|result| result)
    } else {
        Ok(())
    };
    if matches!(mode, Mode::Shutdown | Mode::Failure) {
        if let Err(reason) = protocol {
            log::debug!(
                "[agent-runtime] protocol cleanup failed before owned-tree retirement: {reason}"
            );
        }
        io.finish().await
    } else {
        if io.ended() {
            return Ok(());
        }
        let verified = io.verify(roster).await.map(|unknown| {
            if !unknown.is_empty() {
                io.notice(&unknown);
            }
        });
        protocol.and(verified)
    }
}

fn refused(reason: &str) -> RuntimeFailure {
    RuntimeFailure::Refused(reason.into())
}
async fn page(
    io: &mut impl CleanupIo,
    thread: &str,
    cursor: Option<String>,
) -> Result<(Vec<RuntimeBackgroundProcess>, Option<String>), RuntimeFailure> {
    match io
        .run(RuntimeOperation::BackgroundProcesses {
            thread_id: thread.into(),
            action: RuntimeBackgroundProcessAction::List { cursor },
        })
        .await?
    {
        RuntimeResult::BackgroundProcesses {
            thread_id,
            processes,
            next_cursor,
        } if thread_id == thread => {
            validate_background_processes(&processes)
                .map_err(|error| refused(&error.to_string()))?;
            Ok((processes, next_cursor))
        }
        _ => Err(refused("Invalid background process list result")),
    }
}
async fn list(
    io: &mut impl CleanupIo,
    thread: &str,
) -> Result<Vec<RuntimeBackgroundProcess>, RuntimeFailure> {
    let mut cursor = None;
    let mut seen = HashSet::new();
    let mut ids = HashSet::new();
    let mut all = Vec::new();
    for _ in 0..32 {
        let (processes, next) = page(io, thread, cursor).await?;
        for process in processes {
            if !ids.insert(process.process_id.clone()) || all.len() >= MAX_BACKGROUND_PROCESSES {
                return Err(refused(
                    "Background process roster duplicated an id or exceeded its bound",
                ));
            }
            all.push(process);
        }
        match next {
            None => return Ok(all),
            Some(next) if !next.is_empty() && next.len() <= 4096 && seen.insert(next.clone()) => {
                cursor = Some(next)
            }
            _ => return Err(refused("Background process pagination did not advance")),
        }
    }
    Err(refused("Background process pagination exceeded its bound"))
}
async fn drain(
    io: &mut impl CleanupIo,
    context: &RuntimeContext,
    mode: Mode,
    roster: &mut Vec<RuntimeBackgroundProcess>,
) -> Result<(), RuntimeFailure> {
    let Some(root) = context.thread_id.clone() else {
        return Ok(());
    };
    if matches!(mode, Mode::Shutdown | Mode::Failure)
        && (!context.active_turns.is_empty() || context.active_turn_id.is_some())
    {
        match io
            .run(RuntimeOperation::Interrupt {
                thread_id: root.clone(),
                active_turn_id: context.active_turn_id.clone(),
            })
            .await?
        {
            RuntimeResult::Done => {}
            _ => return Err(refused("Cleanup interrupt was not accepted")),
        }
    }
    let mut threads = vec![root];
    threads.extend(
        context
            .thread_parents
            .iter()
            .filter(|parent| owns_runtime_thread(context, &parent.thread_id))
            .map(|parent| parent.thread_id.clone()),
    );
    for thread in threads {
        let processes = list(io, &thread).await?;
        if roster.len() + processes.len() > MAX_BACKGROUND_PROCESSES {
            return Err(refused(
                "Owned background process roster exceeded its total bound",
            ));
        }
        roster.extend(processes.clone());
        for process in processes {
            match io
                .run(RuntimeOperation::BackgroundProcesses {
                    thread_id: thread.clone(),
                    action: RuntimeBackgroundProcessAction::Terminate {
                        process_id: process.process_id.clone(),
                    },
                })
                .await?
            {
                RuntimeResult::BackgroundProcessTermination {
                    thread_id,
                    process_id,
                    ..
                } if thread_id == thread && process_id == process.process_id => {}
                _ => {
                    return Err(refused(
                        "Background termination returned the wrong identity",
                    ))
                }
            }
        }
        if !list(io, &thread).await?.is_empty() {
            return Err(refused("Background processes remain after termination"));
        }
        if !matches!(
            io.run(RuntimeOperation::BackgroundProcesses {
                thread_id: thread,
                action: RuntimeBackgroundProcessAction::Clean
            })
            .await?,
            RuntimeResult::Done
        ) {
            return Err(refused("Background cleanup was not acknowledged"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
