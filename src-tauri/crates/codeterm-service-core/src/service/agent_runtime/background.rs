use super::{
    engine::{self, Session},
    model::RuntimeFailure,
    process::Process,
    process_boundary::Member,
};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::*;
use std::{collections::HashSet, sync::Arc, time::Duration};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum Mode {
    Interrupt,
    Shutdown,
}

#[async_trait::async_trait]
trait CleanupIo {
    async fn run(&mut self, operation: RuntimeOperation) -> Result<RuntimeResult, RuntimeFailure>;
    async fn verify(&mut self, roster: Vec<RuntimeBackgroundProcess>)
        -> Result<(), RuntimeFailure>;
    async fn finish(&mut self) -> Result<(), RuntimeFailure>;
    fn ended(&self) -> bool {
        false
    }
    fn roster(&mut self, _thread: &str, _processes: &[RuntimeBackgroundProcess]) {}
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
}
#[async_trait::async_trait]
impl CleanupIo for RuntimeIo<'_> {
    async fn run(&mut self, operation: RuntimeOperation) -> Result<RuntimeResult, RuntimeFailure> {
        let cleaned = match &operation {
            RuntimeOperation::BackgroundProcesses {
                thread_id,
                action: RuntimeBackgroundProcessAction::Clean,
            } => Some(thread_id.clone()),
            _ => None,
        };
        let result = engine::run(
            self.svc,
            &self.session.provider,
            self.session,
            self.process,
            operation,
        )
        .await?;
        if let Some(thread) = cleaned {
            if matches!(result, RuntimeResult::Done) {
                self.session
                    .maps
                    .lock()
                    .background_processes
                    .remove(&thread);
            }
        }
        Ok(result)
    }
    async fn verify(
        &mut self,
        roster: Vec<RuntimeBackgroundProcess>,
    ) -> Result<(), RuntimeFailure> {
        self.process
            .verify_interrupt(std::mem::take(&mut self.captured), roster)
            .await
    }
    async fn finish(&mut self) -> Result<(), RuntimeFailure> {
        self.process.finish().await?;
        self.session.maps.lock().background_processes.clear();
        Ok(())
    }
    fn ended(&self) -> bool {
        self.process.terminated()
    }
    fn roster(&mut self, thread: &str, processes: &[RuntimeBackgroundProcess]) {
        self.session
            .maps
            .lock()
            .background_processes
            .insert(thread.into(), processes.into());
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
    };
    coordinate(&mut io, &context, mode, enabled).await
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
    if mode == Mode::Shutdown {
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
        let verified = io.verify(roster).await;
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
    if mode == Mode::Shutdown
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
        io.roster(&thread, &processes);
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
