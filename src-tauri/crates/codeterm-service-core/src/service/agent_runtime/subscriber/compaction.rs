use super::{engine, RuntimeFailure, ServiceContext, Session};
use codeterm_plugin::agent_runtime::{RuntimeContext, RuntimeOperation, RuntimeResult};
use serde_json::{json, Value};
use std::{collections::HashSet, sync::Arc};

#[derive(Default)]
pub(crate) struct CompactionState {
    epoch: Option<u64>,
    completed: HashSet<String>,
    failed: bool,
    restoring: usize,
}

impl CompactionState {
    fn claim(&mut self, epoch: u64, item: &str) -> bool {
        if self.epoch != Some(epoch) {
            self.epoch = Some(epoch);
            self.completed.clear();
            self.failed = false;
            self.restoring = 0;
        }
        let claimed = self.completed.insert(item.into());
        if claimed {
            self.restoring += 1;
        }
        claimed
    }

    pub(crate) fn held(&self, epoch: u64) -> bool {
        self.epoch == Some(epoch) && (self.failed || self.restoring > 0)
    }

    fn finish(&mut self, epoch: u64, failed: bool) {
        if self.epoch == Some(epoch) {
            self.failed = failed;
            self.restoring = self.restoring.saturating_sub(1);
        }
    }
}

fn should_restore(
    expected: &RuntimeContext,
    current: &RuntimeContext,
    binding_valid: bool,
    has_content: bool,
) -> bool {
    has_content
        && binding_valid
        && expected.epoch == current.epoch
        && expected.tab_id == current.tab_id
        && expected.binding_nonce == current.binding_nonce
        && expected.thread_id.is_some()
        && expected.thread_id == current.thread_id
}

fn restore_value(instructions: Option<&str>) -> Value {
    json!({"instructions":instructions,"additionalContext":[]})
}

pub(super) fn schedule(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
    thread: &str,
    item: &str,
) -> Option<crate::async_runtime::JoinHandle<()>> {
    let expected = session.context.lock().clone();
    if expected.epoch != epoch
        || expected.thread_id.as_deref() != Some(thread)
        || !session.compaction.lock().claim(epoch, item)
    {
        return None;
    }
    let owner = svc.clone();
    let session = session.clone();
    let task: std::pin::Pin<Box<dyn std::future::Future<Output = ()> + Send>> =
        Box::pin(async move {
            let result = restore(&owner, &session, &expected).await;
            let current = session.context.lock().clone();
            if !should_restore(
                &expected,
                &current,
                engine::validate_binding(&owner, &session).is_ok(),
                true,
            ) {
                return;
            }
            session.compaction.lock().finish(epoch, result.is_err());
            if let Err(reason) = result {
                session.state.lock().degrade(epoch, &reason);
                engine::publish(&owner, &expected.tab_id, &session);
                owner.emit_state_changed();
                log::debug!(
                    "[agent-runtime] tab={} context restore failed: {reason}; sends held",
                    expected.tab_id
                );
            }
        });
    Some(crate::async_runtime::spawn(task))
}

async fn restore(
    svc: &ServiceContext,
    session: &Arc<Session>,
    expected: &RuntimeContext,
) -> Result<(), RuntimeFailure> {
    let _permit = tokio::time::timeout(std::time::Duration::from_secs(30), session.gate.acquire())
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
        .map_err(|_| RuntimeFailure::Superseded)?;
    let current = session.context.lock().clone();
    if !should_restore(
        expected,
        &current,
        engine::validate_binding(svc, session).is_ok(),
        true,
    ) {
        return Err(RuntimeFailure::Superseded);
    }
    let value = {
        let mux = svc.mux.read();
        let tab = mux
            .get_tab(&expected.tab_id)
            .ok_or(RuntimeFailure::Superseded)?;
        restore_value(
            tab.agent_runtime_config
                .as_ref()
                .and_then(|config| config.get("systemPrompt"))
                .and_then(Value::as_str),
        )
    };
    let has_content = value["instructions"]
        .as_str()
        .is_some_and(|text| !text.is_empty());
    if !should_restore(expected, &current, true, has_content) {
        return Ok(());
    }
    let result = async {
        let process = session
            .process
            .lock()
            .clone()
            .ok_or(RuntimeFailure::PipeEof)?;
        match engine::run(
            svc,
            &session.provider,
            session,
            &process,
            RuntimeOperation::Control {
                thread_id: expected.thread_id.clone(),
                action: "restoreContext".into(),
                value,
            },
        )
        .await?
        {
            RuntimeResult::Done => Ok(()),
            _ => Err(RuntimeFailure::HookFailed(
                "Context restoration returned an invalid result".into(),
            )),
        }
    }
    .await;
    result
}

#[cfg(test)]
#[path = "compaction_tests.rs"]
mod tests;
