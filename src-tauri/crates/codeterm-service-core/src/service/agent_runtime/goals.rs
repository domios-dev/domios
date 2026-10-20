use super::engine;
use crate::agent::chat_types::{ChatMsg, ChatSessionIntent};
use crate::service::ServiceContext;
use codeterm_plugin::agent_runtime::{
    AgentRuntimeEvent, RuntimeContext, RuntimeGoalAction, RuntimeGoalCapability, RuntimeOperation,
    RuntimeResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalBinding {
    pub thread_id: String,
    pub binding_nonce: String,
    pub epoch: u64,
}
impl GoalBinding {
    fn capture(context: &RuntimeContext) -> Result<Self, String> {
        Ok(Self {
            thread_id: context
                .thread_id
                .clone()
                .ok_or("Runtime thread unavailable")?,
            binding_nonce: context.binding_nonce.clone(),
            epoch: context.epoch,
        })
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct GoalRequest {
    pub action: RuntimeGoalAction,
    #[serde(default)]
    pub confirmed: bool,
    pub binding: Option<GoalBinding>,
    pub expected_intent: Option<ChatSessionIntent>,
}

fn authorize(
    request: &GoalRequest,
    capability: &RuntimeGoalCapability,
    current: &GoalBinding,
) -> Result<(), String> {
    if request.action == RuntimeGoalAction::Read {
        return Ok(());
    }
    if !capability.actions.contains(&request.action) {
        return Err("Goal action unsupported by provider".into());
    }
    if !request.confirmed
        || request.binding.as_ref() != Some(current)
        || request.expected_intent.is_none()
    {
        return Err("Goal action requires confirmation of the current bound goal".into());
    }
    Ok(())
}
fn result_intent(
    result: RuntimeResult,
    binding: &GoalBinding,
) -> Result<Option<ChatSessionIntent>, String> {
    match result {
        RuntimeResult::Goal { thread_id, intent } if thread_id == binding.thread_id => Ok(intent),
        _ => Err("Runtime did not confirm the bound goal".into()),
    }
}
fn operation(binding: &GoalBinding, action: RuntimeGoalAction) -> RuntimeOperation {
    RuntimeOperation::Control {
        thread_id: Some(binding.thread_id.clone()),
        action: "goal".into(),
        value: json!({"action":action}),
    }
}

pub async fn operate(
    svc: &ServiceContext,
    tab: &str,
    request: GoalRequest,
) -> Result<Value, String> {
    let Some(session) = engine::for_tab(tab) else {
        return if request.action == RuntimeGoalAction::Read {
            Ok(json!({"supported":false}))
        } else {
            Err("Runtime binding unavailable".into())
        };
    };
    engine::validate_binding(svc, &session).map_err(|e| e.to_string())?;
    let capability = session
        .adapter
        .lock()
        .as_ref()
        .and_then(|provider| provider.agent_runtime())
        .and_then(|cap| cap.goals);
    let Some(capability) = capability else {
        return if request.action == RuntimeGoalAction::Read {
            Ok(json!({"supported":false}))
        } else {
            Err("Goal controls unsupported by provider".into())
        };
    };
    let _permit = session
        .gate
        .try_acquire()
        .map_err(|_| "Runtime operation already in progress")?;
    let binding = GoalBinding::capture(&session.context.lock())?;
    authorize(&request, &capability, &binding)?;
    let process = session
        .process
        .lock()
        .clone()
        .ok_or("Runtime unavailable")?;
    let run = |action| {
        engine::run(
            svc,
            &session.provider,
            &session,
            &process,
            operation(&binding, action),
        )
    };
    let intent = if request.action == RuntimeGoalAction::Read {
        result_intent(
            run(RuntimeGoalAction::Read)
                .await
                .map_err(|e| e.to_string())?,
            &binding,
        )?
    } else {
        let current = result_intent(
            run(RuntimeGoalAction::Read)
                .await
                .map_err(|e| e.to_string())?,
            &binding,
        )?;
        if current != request.expected_intent {
            return Err("Goal changed; refresh and confirm again".into());
        }
        engine::validate_binding(svc, &session).map_err(|e| e.to_string())?;
        if GoalBinding::capture(&session.context.lock())? != binding {
            return Err("Goal binding superseded".into());
        }
        result_intent(
            run(request.action).await.map_err(|e| e.to_string())?,
            &binding,
        )?
    };
    engine::validate_binding(svc, &session).map_err(|e| e.to_string())?;
    if GoalBinding::capture(&session.context.lock())? != binding {
        return Err("Goal binding superseded".into());
    }
    validate_outcome(request.action, &intent)?;
    let row = ChatMsg {
        id: "thread:goal".into(),
        kind: "custom".into(),
        session_intent: Some(intent.clone().unwrap_or(ChatSessionIntent {
            title: None,
            goal: None,
            goal_status: None,
        })),
        ..ChatMsg::default()
    };
    super::subscriber::publish_events(
        svc,
        &session,
        binding.epoch,
        vec![AgentRuntimeEvent::ChatItem {
            thread_id: binding.thread_id.clone(),
            item: row,
        }],
        chrono::Utc::now().timestamp_millis().max(0) as u64,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(json!({"supported":true,"binding":binding,"intent":intent,"actions":capability.actions}))
}

fn validate_outcome(
    action: RuntimeGoalAction,
    intent: &Option<ChatSessionIntent>,
) -> Result<(), String> {
    let valid = match action {
        RuntimeGoalAction::Read => true,
        RuntimeGoalAction::Cancel => intent.is_none(),
        _ => {
            intent
                .as_ref()
                .and_then(|intent| intent.goal_status.as_deref())
                == Some(match action {
                    RuntimeGoalAction::Pause => "paused",
                    RuntimeGoalAction::Resume => "active",
                    RuntimeGoalAction::Complete => "complete",
                    _ => unreachable!(),
                })
        }
    };
    if valid {
        Ok(())
    } else {
        Err("Provider did not confirm the requested goal outcome".into())
    }
}

#[cfg(test)]
mod tests;
