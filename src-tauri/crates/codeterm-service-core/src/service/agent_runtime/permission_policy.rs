use super::client_bridge::gate::{self, Access, Decision, Kind, SessionGrants};
use super::engine::{self, Session};
use super::model::RuntimeFailure;
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    PromptAnswer, RpcId, RuntimeOperation, RuntimePermission, RuntimePermissionAction,
    RuntimePermissionDecision,
};
use codeterm_types::pty::PendingPrompt;
use std::{sync::Arc, time::Duration};

const PERMIT_DEADLINE: Duration = Duration::from_secs(30);

/// The option the launch policy picks for an agent permission request; `None` asks the user.
pub(super) fn decide(
    access: Access,
    permission: &RuntimePermission,
    prompt: &PendingPrompt,
) -> Option<String> {
    let kind = match permission.action {
        RuntimePermissionAction::Read => Kind::SensitiveRead,
        RuntimePermissionAction::Write => Kind::Write,
        RuntimePermissionAction::Command | RuntimePermissionAction::Other => Kind::Command,
    };
    let preference: &[RuntimePermissionDecision] =
        match (SessionGrants::default().decide(access, kind), access) {
            (Decision::Allow, Access::Full) => &[
                RuntimePermissionDecision::AllowAlways,
                RuntimePermissionDecision::AllowOnce,
            ],
            // A sandbox never escalates through an agent prompt; its runtime mode bounds the work.
            (Decision::Refuse, _) => &[
                RuntimePermissionDecision::RejectOnce,
                RuntimePermissionDecision::RejectAlways,
            ],
            _ => return None,
        };
    preference.iter().find_map(|decision| {
        permission
            .options
            .iter()
            .find(|option| {
                option.decision == *decision
                    && prompt
                        .options
                        .iter()
                        .any(|shown| shown.value == option.value)
            })
            .map(|option| option.value.clone())
    })
}

/// Answers an agent permission request from the tab's launch policy; false leaves it to the user.
pub(super) fn answer(
    svc: &ServiceContext,
    session: &Arc<Session>,
    request: &RpcId,
    prompt: &PendingPrompt,
    permission: Option<&RuntimePermission>,
) -> bool {
    let Some(permission) = permission else {
        return false;
    };
    let context = session.context.lock().clone();
    let Ok(access) = gate::access(svc, &context) else {
        return false;
    };
    let Some(value) = decide(access, permission, prompt) else {
        return false;
    };
    let owner = svc.clone();
    let session = session.clone();
    let request = request.clone();
    let prompt = prompt.clone();
    crate::async_runtime::spawn(async move {
        if let Err(error) = reply(&owner, &session, context.epoch, &request, value).await {
            log::warn!(
                "[runtime-permission] tab={} automatic answer stopped: {error}",
                context.tab_id
            );
            if session.prompts.lock().contains_key(&request) {
                super::prompt_answers::publish(&owner, &session, request, prompt);
            }
        }
    });
    true
}

async fn reply(
    svc: &ServiceContext,
    session: &Arc<Session>,
    epoch: u64,
    request: &RpcId,
    value: String,
) -> Result<(), RuntimeFailure> {
    let _permit = tokio::time::timeout(PERMIT_DEADLINE, session.gate.acquire())
        .await
        .map_err(|_| RuntimeFailure::Deadline)?
        .map_err(|_| RuntimeFailure::Superseded)?;
    if session.context.lock().epoch != epoch || !session.prompts.lock().contains_key(request) {
        return Ok(());
    }
    let process = session
        .process
        .lock()
        .clone()
        .ok_or(RuntimeFailure::Superseded)?;
    let operation = RuntimeOperation::Answer {
        request_id: request.clone(),
        answer: PromptAnswer::Choice {
            values: vec![value],
        },
    };
    engine::run(svc, &session.provider, session, &process, operation)
        .await
        .map(drop)
}

/// Choosing only declared reject options never escalates, so the sandbox checks it as a cancel.
pub(super) fn declined(
    session: &Session,
    operation: &RuntimeOperation,
) -> Option<RuntimeOperation> {
    let RuntimeOperation::Answer {
        request_id,
        answer: PromptAnswer::Choice { values },
    } = operation
    else {
        return None;
    };
    let prompts = session.prompts.lock();
    let permission = prompts.get(request_id)?.permission.as_ref()?;
    rejects(permission, values).then(|| RuntimeOperation::Answer {
        request_id: request_id.clone(),
        answer: PromptAnswer::Cancel,
    })
}

fn rejects(permission: &RuntimePermission, values: &[String]) -> bool {
    !values.is_empty()
        && values.iter().all(|value| {
            permission.options.iter().any(|option| {
                &option.value == value
                    && matches!(
                        option.decision,
                        RuntimePermissionDecision::RejectOnce
                            | RuntimePermissionDecision::RejectAlways
                    )
            })
        })
}

#[cfg(test)]
mod tests;
