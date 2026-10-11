use serde_json::{json, Value};

use crate::multiplexer::state::MultiplexerState;
use crate::sentinel::{
    machine::Route,
    provenance::{DecidedBy, Decision},
    store::Sentinel,
};

const MAX_TEXT_BYTES: usize = 4096;
const MAX_DEFINITION_BYTES: usize = 64 * 1024;
const MAX_BUCKETS: usize = 64;
const MAX_COMMANDS: usize = 128;

fn text(input: &str, limit: usize) -> (String, bool) {
    let redacted = crate::service::supporter_tab_state::redact_text(input);
    let mut end = redacted.len().min(limit);
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    (redacted[..end].to_owned(), end < redacted.len())
}

pub(super) fn definition(job: &Sentinel) -> Value {
    if job.pipeline().is_some() {
        return json!({"unavailable":"pipeline_definition_not_representable_in_v1"});
    }
    let (probe, probe_truncated) = text(&job.probe(), MAX_TEXT_BYTES);
    let (question, question_truncated) = text(&job.question, MAX_TEXT_BYTES);
    if question_truncated || job.buckets.len() > MAX_BUCKETS {
        return json!({"unavailable":"definition_exceeds_v1_bounds"});
    }
    let mut buckets = Vec::new();
    for bucket in &job.buckets {
        let pattern = job
            .matches
            .get(bucket)
            .map(|pattern| text(pattern, MAX_TEXT_BYTES));
        if pattern.as_ref().is_some_and(|(_, truncated)| *truncated) || bucket.len() > 256 {
            return json!({"unavailable":"definition_exceeds_v1_bounds"});
        }
        buckets.push(json!({"name":text(bucket,256).0,"pattern":pattern.map(|(text,_)| text)}));
    }
    let mut commands = Vec::new();
    let other = "other".to_owned();
    for (bucket, handlers) in job
        .routes
        .iter()
        .chain(std::iter::once((&other, &job.on_other)))
    {
        for handler in handlers {
            if let Route::Command(parameters) = &handler.action {
                if commands.len() == MAX_COMMANDS {
                    return json!({"unavailable":"definition_exceeds_v1_bounds"});
                }
                let (command, command_truncated) = text(&parameters.command, MAX_TEXT_BYTES);
                commands.push(
                    json!({"bucket":text(bucket,256).0,"handler_id":text(&handler.id,256).0,
                    "label":text(&parameters.label,256).0,"command":command,
                    "command_truncated":command_truncated,"timeout_ms":parameters.timeout_ms}),
                );
            }
        }
    }
    let definition = json!({"probe":(!job.probe().is_empty()).then_some(probe),"probe_truncated":probe_truncated,"model_classification":job.model_classification,"decision_kind":if job.matches.is_empty() && job.model_classification {"agent"} else {"pattern"},"command_language":if cfg!(windows) {"powershell"} else {"bash"},"language":if cfg!(windows) {"powershell"} else {"bash"},
        "question":question,"buckets":buckets,"commands":commands});
    if definition.to_string().len() > MAX_DEFINITION_BYTES {
        json!({"unavailable":"definition_exceeds_v1_bounds"})
    } else {
        definition
    }
}

pub(super) fn project(
    job: &Sentinel,
    mux: &MultiplexerState,
    workspace: &str,
    team: Option<&str>,
    now_ms: u64,
    executing: bool,
) -> Value {
    let exact_scope = job.workspace_id == workspace && job.team_id.as_deref() == team;
    let creator_name = mux
        .get_tab(&job.creator_tab_id)
        .filter(|_| mux.find_workspace_for_tab(&job.creator_tab_id).as_deref() == Some(workspace))
        .map(|tab| text(&tab.title, 256).0);
    let team_name = job
        .team_id
        .as_deref()
        .and_then(|id| mux.get_team(id))
        .filter(|team| team.workspace_id == workspace)
        .and_then(|team| team.name.as_deref())
        .map(|name| text(name, 256).0);
    let history_complete = job
        .transitions
        .iter()
        .all(|transition| transition.decision.as_ref().is_some_and(Decision::valid));
    let adapter_used = history_complete.then(|| {
        job.transitions.iter().any(|transition| {
            transition
                .decision
                .as_ref()
                .is_some_and(|decision| decision.decided_by == DecidedBy::Adapter)
        })
    });
    let scheduled = job.pipeline().is_none()
        && job.input().is_none()
        && !executing
        && !job.machine.paused
        && !job.machine.stopped
        && job.machine.held.is_none()
        && job
            .expires_at
            .is_none_or(|at| now_ms < at && job.next_run < at)
        && !(matches!(
            job.observe().cloned(),
            Some(crate::sentinel::source::TabBinding::CronRun(_))
        ) && job.run.is_none());
    json!({"version":1,"scope":{"workspace_id":job.workspace_id,"team_id":job.team_id,"node_id":job.id},
        "runs_since_transition":job.progress.runs_since_transition,"last_probe_at":job.progress.last_probe_at,"probe_state":job.progress.probe_state,
        "origin":if job.agent_authored {"agent"} else {"unknown"},
        "creator":if job.creator_tab_id.is_empty() {Value::Null} else {json!({"tab_id":job.creator_tab_id,"name":creator_name})},
        "team":job.team_id.as_ref().map(|id| json!({"id":id,"name":team_name})),
        "observe_target":if job.input().is_some() {None} else {job.observe()},"next_check_at_ms":scheduled.then_some(job.next_run),
        "definition":if exact_scope {definition(job)} else {json!({"unavailable":"team_view_required"})},
        "decisions":{"history_complete":history_complete,"adapter_used":adapter_used}})
}

pub(super) fn missing(identity: &super::Identity) -> Value {
    json!({"version":1,"scope":{"workspace_id":identity.workspace_id,"team_id":identity.team_id,"node_id":identity.id},
        "origin":"unknown","creator":identity.owner_tab_id.as_ref().map(|id| json!({"tab_id":id,"name":null})),
        "team":identity.team_id.as_ref().map(|id| json!({"id":id,"name":null})),
        "observe_target":null,"next_check_at_ms":null,"definition":{"unavailable":"node_missing"},
        "decisions":{"history_complete":false,"adapter_used":null}})
}

pub(super) fn project_decision(result: &mut Value, decision: Option<&Decision>) {
    let decision = decision.filter(|decision| decision.valid());
    result["decided_by"] = decision
        .map(|decision| json!(decision.decided_by))
        .unwrap_or(Value::Null);
    result["adapter_id"] = decision
        .and_then(|decision| decision.adapter_id.as_deref())
        .map(|id| json!(text(id, 256).0))
        .unwrap_or(Value::Null);
    result["model"] = decision
        .and_then(|decision| decision.model.as_deref())
        .map(|model| json!(text(model, 256).0))
        .unwrap_or(Value::Null);
    result["confidence"] = decision
        .and_then(|decision| decision.confidence)
        .map(|confidence| json!(confidence))
        .unwrap_or(Value::Null);
}

pub(crate) fn configured_fallback(adapter_id: Option<&str>, model: Option<&str>) -> Value {
    adapter_id
        .map(|id| json!({"adapter_id":text(id,256).0,"model":model.map(|model| text(model,256).0)}))
        .unwrap_or(Value::Null)
}

#[cfg(test)]
mod tests;
