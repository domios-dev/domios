use super::*;
use codeterm_types::plan::ExecutionIntent;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) enum Kind {
    SensitiveRead,
    Write,
    Command,
}

impl Kind {
    pub(super) fn of(request: &Request) -> Option<Self> {
        match request {
            Request::Read { .. } => Some(Self::SensitiveRead),
            Request::Write { .. } => Some(Self::Write),
            Request::Create { .. } => Some(Self::Command),
            _ => None,
        }
    }
}

#[derive(Clone, Copy)]
pub(in crate::service::agent_runtime) enum Access {
    Full,
    WorkspaceWrite,
    ReadOnly,
    Interactive,
}

#[derive(Debug, PartialEq, Eq)]
pub(in crate::service::agent_runtime) enum Decision {
    Allow,
    Prompt,
    Refuse,
}

#[derive(Default)]
pub(in crate::service::agent_runtime) struct SessionGrants {
    identity: Option<(String, u64, String, Option<String>)>,
    read: bool,
    write: bool,
    command: bool,
}

impl SessionGrants {
    pub(super) fn bind(&mut self, context: &RuntimeContext) {
        let identity = (
            context.tab_id.clone(),
            context.epoch,
            context.binding_nonce.clone(),
            context.thread_id.clone(),
        );
        if self.identity.as_ref() != Some(&identity) {
            *self = Self {
                identity: Some(identity),
                ..Self::default()
            };
        }
    }
    pub(super) fn grant(&mut self, kind: Kind) {
        *match kind {
            Kind::SensitiveRead => &mut self.read,
            Kind::Write => &mut self.write,
            Kind::Command => &mut self.command,
        } = true;
    }

    pub(in crate::service::agent_runtime) fn decide(&self, access: Access, kind: Kind) -> Decision {
        match (access, kind) {
            (Access::ReadOnly, Kind::Write | Kind::Command)
            | (Access::WorkspaceWrite, Kind::Command) => Decision::Refuse,
            (Access::Full, _) | (Access::WorkspaceWrite, Kind::Write) => Decision::Allow,
            _ if match kind {
                Kind::SensitiveRead => self.read,
                Kind::Write => self.write,
                Kind::Command => self.command,
            } =>
            {
                Decision::Allow
            }
            _ => Decision::Prompt,
        }
    }
}

pub(in crate::service::agent_runtime) fn access(
    svc: &ServiceContext,
    context: &RuntimeContext,
) -> Result<Access, String> {
    let mux = svc.mux.read();
    let tab = mux
        .get_tab(&context.tab_id)
        .ok_or("Client bridge tab unavailable")?;
    if tab.session_binding_nonce.as_deref() != Some(context.binding_nonce.as_str()) {
        return Err("Client bridge binding changed".into());
    }
    if let Some(policy) = &tab.restorable_launch.execution_sandbox {
        crate::service::execution_sandbox::validate_runtime_settings(
            policy,
            &context.settings,
            &context.cwd,
        )?;
        return Ok(if policy.intent == ExecutionIntent::Mutating {
            Access::WorkspaceWrite
        } else {
            Access::ReadOnly
        });
    }
    drop(mux);
    Ok(
        if svc.full_access_with_settings(&context.tab_id, Some(&context.settings)) {
            Access::Full
        } else {
            Access::Interactive
        },
    )
}

pub(super) fn prompt(question: String) -> Result<PendingPrompt, String> {
    serde_json::from_value(json!({"question":question,"options":[
        {"label":"Allow once","value":"allow_once"},
        {"label":"Allow for this session","value":"allow_session"},
        {"label":"Reject","value":"reject_once"}
    ]}))
    .map_err(|error| error.to_string())
}

pub(super) fn session_answer(answer: &PromptAnswer) -> bool {
    matches!(answer, PromptAnswer::Choice { values } if values == &["allow_session"])
}

pub(super) fn allowed_answer(answer: &PromptAnswer) -> bool {
    session_answer(answer)
        || matches!(answer, PromptAnswer::Choice { values } if values == &["allow_once"])
}

pub(super) fn record_session_grant(
    bridge: &Bridge,
    context: &RuntimeContext,
    pending: &Pending,
    answer: &PromptAnswer,
) -> Result<(), String> {
    let mut state = bridge.state.lock();
    if state.closed || state.generation != pending.generation || state.session != context.thread_id
    {
        return Err("Approval session expired".into());
    }
    if session_answer(answer) {
        state.session_grants.bind(context);
        if let Some(kind) = Kind::of(&pending.request) {
            state.session_grants.grant(kind);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
