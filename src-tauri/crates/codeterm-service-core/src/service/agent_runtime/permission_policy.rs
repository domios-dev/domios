use super::client_bridge::gate::{self, Access, Decision, Kind, SessionGrants};
use super::engine::{self, Session};
use super::model::RuntimeFailure;
use crate::multiplexer::types::HostApprovalChoice;
use crate::service::{MuxEmitCause, ServiceContext};
use codeterm_plugin::agent_runtime::{
    PromptAnswer, RpcId, RuntimeOperation, RuntimePermission, RuntimePermissionAction,
    RuntimePermissionDecision, RuntimeSettings,
};
use codeterm_types::pty::PendingPrompt;
use std::{sync::Arc, time::Duration};

const PERMIT_DEADLINE: Duration = Duration::from_secs(30);
pub(super) const HOST_APPROVALS_KEY: &str = "hostApprovals";
pub(super) const SESSION_MODE_KEY: &str = "sessionMode";

/// What the full-access decision reads; plain inputs keep the rule testable.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct AccessFacts<'a> {
    pub recorded: bool,
    pub host_approvals: bool,
    pub grant_bound: bool,
    pub native_mode: Option<&'a str>,
    /// `None` before the tab has a session to report the modes it offers.
    pub native_advertised: Option<bool>,
    pub selected_mode: Option<&'a str>,
    pub choice: Option<HostApprovalChoice>,
}

/// Whether the owner's effective choice is "no permission questions" for this tab.
///
/// Reads only what the owner or the launch policy decided. The agent's live mode is deliberately
/// absent: an agent can still ask in its approve-all mode, and it must not lift its own Ask by
/// changing mode. An unset choice is full access, the same default every launch has.
pub(super) fn full_access_in_effect(facts: AccessFacts<'_>) -> bool {
    if facts.grant_bound || !facts.host_approvals {
        return facts.recorded;
    }
    if let (Some(native), Some(selected)) = (facts.native_mode, facts.selected_mode) {
        if facts.native_advertised != Some(false) {
            return selected == native;
        }
    }
    facts.choice != Some(HostApprovalChoice::Ask)
}

/// Mode controls decide approvals, and the host cannot recognise an approve-all mode a profile
/// leaves undeclared, so every mode change is the owner's, whichever shape the agent exposes.
fn owner_only_control(settings: Option<&RuntimeSettings>, mode: bool, id: &str) -> bool {
    mode || settings
        .and_then(|settings| settings.config_options.as_ref())
        .is_some_and(|options| {
            options
                .iter()
                .any(|option| option.id == id && option.category.as_deref() == Some("mode"))
        })
}

/// The selected launch profile's declared approve-everything session mode.
pub(super) fn declared_full_access_mode(
    svc: &ServiceContext,
    provider: &str,
    profile: &str,
) -> Option<String> {
    svc.agent_registry
        .with_provider(provider, |provider| {
            provider
                .agent_runtime()?
                .launch_profiles?
                .get(profile)?
                .full_access_mode
                .clone()
        })
        .flatten()
}

pub(super) fn declares_host_approvals(svc: &ServiceContext, provider: &str) -> bool {
    svc.agent_registry
        .with_provider(provider, |provider| {
            provider
                .agent_runtime()
                .is_some_and(|runtime| runtime.host_approvals)
        })
        .unwrap_or(false)
}

struct TabPolicy {
    provider: String,
    profile: Option<String>,
    recorded: bool,
    sandboxed: bool,
    grant_bound: bool,
    selected_mode: Option<String>,
    choice: Option<HostApprovalChoice>,
}

fn tab_policy(svc: &ServiceContext, tab_id: &str) -> Option<TabPolicy> {
    let mux = svc.mux.read();
    let tab = mux.get_tab(tab_id)?;
    Some(TabPolicy {
        provider: tab.agent_type.clone()?,
        profile: tab
            .agent_runtime_config
            .as_ref()
            .and_then(|config| config.get("launchProfile"))
            .and_then(|profile| profile.as_str())
            .map(str::to_owned),
        recorded: tab.restorable_launch.permission_bypass_requested,
        sandboxed: tab.restorable_launch.execution_sandbox.is_some(),
        grant_bound: tab.neighbor_authority.is_some(),
        selected_mode: tab.restorable_launch.selected_session_mode.clone(),
        choice: tab.restorable_launch.host_approval_choice,
    })
}

fn resolve(
    svc: &ServiceContext,
    policy: &TabPolicy,
    recorded: bool,
    settings: Option<&RuntimeSettings>,
) -> bool {
    if policy.sandboxed {
        return false;
    }
    let native = policy
        .profile
        .as_deref()
        .and_then(|profile| declared_full_access_mode(svc, &policy.provider, profile));
    full_access_in_effect(AccessFacts {
        recorded,
        host_approvals: declares_host_approvals(svc, &policy.provider),
        grant_bound: policy.grant_bound,
        native_mode: native.as_deref(),
        native_advertised: native_advertised(native.as_deref(), settings),
        selected_mode: policy.selected_mode.as_deref(),
        choice: policy.choice,
    })
}

fn native_advertised(native: Option<&str>, settings: Option<&RuntimeSettings>) -> Option<bool> {
    let native = native?;
    let settings = settings?;
    let as_mode = settings
        .modes
        .as_ref()
        .is_some_and(|modes| modes.iter().any(|mode| mode.id == native));
    let as_option = settings.config_options.as_ref().is_some_and(|options| {
        options.iter().any(|option| {
            option.category.as_deref() == Some("mode")
                && option.options.iter().any(|value| value.value == native)
        })
    });
    Some(as_mode || as_option)
}

/// A launch resolves from what the launch was granted, before any session reports its modes.
pub(super) fn launch_full_access(svc: &ServiceContext, tab_id: &str, granted: bool) -> bool {
    tab_policy(svc, tab_id).map_or(granted, |policy| resolve(svc, &policy, granted, None))
}

/// The view the picker reads while the tab sleeps; `locked` means confinement owns the answer.
pub(super) fn host_approvals_view(
    svc: &ServiceContext,
    tab_id: &str,
    full_access: bool,
) -> Option<&'static str> {
    let policy = tab_policy(svc, tab_id)?;
    declares_host_approvals(svc, &policy.provider).then_some(
        if policy.sandboxed || policy.grant_bound {
            "locked"
        } else if full_access {
            "allow_all"
        } else {
            "ask"
        },
    )
}

impl ServiceContext {
    /// The tab's effective full access now, from its live session when it has one.
    pub(crate) fn effective_full_access(&self, tab_id: &str) -> bool {
        let settings =
            engine::for_tab(tab_id).map(|session| session.context.lock().settings.clone());
        self.full_access_with_settings(tab_id, settings.as_ref())
    }

    pub(super) fn full_access_with_settings(
        &self,
        tab_id: &str,
        settings: Option<&RuntimeSettings>,
    ) -> bool {
        tab_policy(self, tab_id)
            .is_some_and(|policy| resolve(self, &policy, policy.recorded, settings))
    }

    /// Whether this control change is reserved for the owner; see [`owner_only_control`].
    pub fn runtime_control_requires_owner(&self, tab_id: &str, mode: bool, id: &str) -> bool {
        let settings =
            engine::for_tab(tab_id).map(|session| session.context.lock().settings.clone());
        owner_only_control(settings.as_ref(), mode, id)
    }

    /// The owner's live switch for agents with no approve-all mode of their own.
    pub fn set_tab_host_approvals(&self, tab_id: &str, allow_all: bool) -> Result<bool, String> {
        if let Some(refusal) = self.tab_write_lock(tab_id) {
            return Err(refusal.to_string());
        }
        let policy = tab_policy(self, tab_id).ok_or("Agent tab unavailable")?;
        if !declares_host_approvals(self, &policy.provider) {
            return Err("This agent does not support host approvals".into());
        }
        if policy.sandboxed || policy.grant_bound {
            return Err("This tab's confinement decides its permissions".into());
        }
        let settings =
            engine::for_tab(tab_id).map(|session| session.context.lock().settings.clone());
        let native = policy
            .profile
            .as_deref()
            .and_then(|profile| declared_full_access_mode(self, &policy.provider, profile));
        if native.is_some()
            && native_advertised(native.as_deref(), settings.as_ref()) != Some(false)
        {
            return Err("This agent's own mode is its permission switch".into());
        }
        let choice = if allow_all {
            HostApprovalChoice::AllowAll
        } else {
            HostApprovalChoice::Ask
        };
        let previous =
            self.record_host_policy(tab_id, |launch| launch.host_approval_choice.replace(choice))?;
        if let Err(error) = crate::service::session_persist::persist_session_after_mutation(
            self,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        ) {
            self.record_host_policy(tab_id, |launch| launch.host_approval_choice = previous)?;
            return Err(error);
        }
        super::prompt_answers::settle_after_policy_change(self, tab_id);
        self.emit_state_changed(MuxEmitCause::Prompt);
        Ok(allow_all)
    }

    /// Writes the choice, then every projection of it, so the tab never reports a stale state.
    fn record_host_policy<T>(
        &self,
        tab_id: &str,
        change: impl FnOnce(&mut crate::multiplexer::types::RestorableLaunchParameters) -> T,
    ) -> Result<T, String> {
        let result = {
            let mut mux = self.mux.write();
            let tab = mux.get_tab_mut(tab_id).ok_or("Agent tab unavailable")?;
            change(&mut tab.restorable_launch)
        };
        let full_access = self.effective_full_access(tab_id);
        let view = host_approvals_view(self, tab_id, full_access);
        let mut mux = self.mux.write();
        let tab = mux.get_tab_mut(tab_id).ok_or("Agent tab unavailable")?;
        tab.restorable_launch.permission_bypass_requested = full_access;
        tab.auto_approve = full_access;
        let config = tab
            .agent_runtime_config
            .get_or_insert_with(|| serde_json::json!({}));
        if let (Some(config), Some(view)) = (config.as_object_mut(), view) {
            config.insert(HOST_APPROVALS_KEY.into(), serde_json::json!(view));
        }
        Ok(result)
    }
}

/// The owner's chosen mode survives restore; the agent's load resets it to its own default.
/// Mode changes reach this only from the owner: the controls route refuses every agent caller.
pub(super) fn record_selected_mode(svc: &ServiceContext, session: &Arc<Session>, mode: &str) {
    let tab_id = session.context.lock().tab_id.clone();
    let Some(policy) = tab_policy(svc, &tab_id) else {
        return;
    };
    if !declares_host_approvals(svc, &policy.provider) || policy.sandboxed || policy.grant_bound {
        return;
    }
    let recorded = svc.record_host_policy(&tab_id, |launch| {
        launch.selected_session_mode = Some(mode.to_owned());
    });
    let persisted = recorded.and_then(|()| {
        crate::service::session_persist::persist_session_after_mutation(
            svc,
            crate::service::session_persist::SessionPersistUrgency::Prompt,
        )
    });
    if let Err(error) = persisted {
        log::warn!("[runtime-permission] tab={tab_id} chosen mode was not persisted: {error}");
    }
    super::prompt_answers::settle_after_policy_change(svc, &tab_id);
    svc.emit_state_changed(MuxEmitCause::Prompt);
}

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
    engine::run(svc, &session.provider, session, &process, operation).await?;
    // A prompt already shown to the owner leaves the screen once the new setting answered it.
    if !session.prompts.lock().contains_key(request) {
        super::prompt_answers::resolve(svc, session, request);
    }
    Ok(())
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
