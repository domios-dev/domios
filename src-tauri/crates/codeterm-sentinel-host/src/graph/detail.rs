use serde_json::json;
#[cfg(test)]
use serde_json::Value;

use super::MuxView;
use crate::{
    machine::Route,
    provenance::{DecidedBy, Decision},
    store::Sentinel,
};
use codeterm_sentinel::graph_contracts::{
    AvailableDefinition, Definition, DefinitionBucket, DefinitionCommand, Detail, DetailCreator,
    DetailDecisions, DetailScope, DetailTeam,
};
#[cfg(test)]
use codeterm_service_core::multiplexer::state::MultiplexerState;

const MAX_TEXT_BYTES: usize = 4096;
const MAX_DEFINITION_BYTES: usize = 64 * 1024;
const MAX_BUCKETS: usize = 64;
const MAX_COMMANDS: usize = 128;

fn text(input: &str, limit: usize) -> (String, bool) {
    let redacted = codeterm_service_core::service::supporter_tab_state::redact_text(input);
    let mut end = redacted.len().min(limit);
    while !redacted.is_char_boundary(end) {
        end -= 1;
    }
    (redacted[..end].to_owned(), end < redacted.len())
}

pub(super) fn definition(job: &Sentinel) -> Definition {
    if job.pipeline().is_some() {
        return Definition::Unavailable {
            unavailable: "pipeline_definition_not_representable_in_v1".into(),
        };
    }
    let (probe, probe_truncated) = text(&job.probe(), MAX_TEXT_BYTES);
    let (question, question_truncated) = text(&job.question, MAX_TEXT_BYTES);
    if question_truncated || job.buckets.len() > MAX_BUCKETS {
        return Definition::Unavailable {
            unavailable: "definition_exceeds_v1_bounds".into(),
        };
    }
    let mut buckets = Vec::new();
    for bucket in &job.buckets {
        let pattern = job
            .matches
            .get(bucket)
            .map(|pattern| text(pattern, MAX_TEXT_BYTES));
        if pattern.as_ref().is_some_and(|(_, truncated)| *truncated) || bucket.len() > 256 {
            return Definition::Unavailable {
                unavailable: "definition_exceeds_v1_bounds".into(),
            };
        }
        buckets.push(DefinitionBucket {
            name: text(bucket, 256).0,
            pattern: pattern.map(|(text, _)| text),
        });
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
                    return Definition::Unavailable {
                        unavailable: "definition_exceeds_v1_bounds".into(),
                    };
                }
                let (command, command_truncated) = text(&parameters.command, MAX_TEXT_BYTES);
                commands.push(DefinitionCommand {
                    bucket: text(bucket, 256).0,
                    handler_id: text(&handler.id, 256).0,
                    label: text(&parameters.label, 256).0,
                    command,
                    command_truncated,
                    timeout_ms: parameters.timeout_ms,
                });
            }
        }
    }
    let language = if cfg!(windows) { "powershell" } else { "bash" };
    let definition = Definition::Available(AvailableDefinition {
        probe: (!job.probe().is_empty()).then_some(probe),
        probe_truncated,
        model_classification: job.model_classification,
        decision_kind: if job.matches.is_empty() && job.model_classification {
            "agent"
        } else {
            "pattern"
        }
        .into(),
        command_language: language.into(),
        language: language.into(),
        question,
        buckets,
        commands,
    });
    if json!(definition).to_string().len() > MAX_DEFINITION_BYTES {
        Definition::Unavailable {
            unavailable: "definition_exceeds_v1_bounds".into(),
        }
    } else {
        definition
    }
}

pub(super) fn project(
    job: &Sentinel,
    mux: &dyn MuxView,
    workspace: &str,
    team: Option<&str>,
    now_ms: u64,
    executing: bool,
) -> Detail {
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
        && job.admission.permits_steps()
        && job
            .expires_at
            .is_none_or(|at| now_ms < at && job.next_run < at)
        && !(matches!(
            job.observe().cloned(),
            Some(crate::source::TabBinding::CronRun(_))
        ) && job.run.is_none());
    Detail {
        version: 1,
        scope: DetailScope {
            workspace_id: job.workspace_id.clone(),
            team_id: job.team_id.clone(),
            node_id: job.id.clone(),
        },
        runs_since_transition: Some(job.progress.runs_since_transition),
        last_probe_at: Some(job.progress.last_probe_at),
        probe_state: Some(job.progress.probe_state.clone()),
        origin: if job.agent_authored {
            "agent"
        } else {
            "unknown"
        }
        .into(),
        creator: (!job.creator_tab_id.is_empty()).then(|| DetailCreator {
            tab_id: job.creator_tab_id.clone(),
            name: creator_name,
        }),
        team: job.team_id.as_ref().map(|id| DetailTeam {
            id: id.clone(),
            name: team_name,
        }),
        observe_target: if job.input().is_some() {
            None
        } else {
            job.observe().cloned()
        },
        next_check_at_ms: scheduled.then_some(job.next_run),
        definition: if exact_scope {
            definition(job)
        } else {
            Definition::Unavailable {
                unavailable: "team_view_required".into(),
            }
        },
        decisions: DetailDecisions {
            history_complete,
            adapter_used,
        },
    }
}

pub(super) fn missing(identity: &super::Identity) -> Detail {
    Detail {
        version: 1,
        scope: DetailScope {
            workspace_id: identity.workspace_id.clone(),
            team_id: identity.team_id.clone(),
            node_id: identity.id.clone(),
        },
        origin: "unknown".into(),
        creator: identity.owner_tab_id.as_ref().map(|id| DetailCreator {
            tab_id: id.clone(),
            name: None,
        }),
        team: identity.team_id.as_ref().map(|id| DetailTeam {
            id: id.clone(),
            name: None,
        }),
        observe_target: None,
        next_check_at_ms: None,
        definition: Definition::Unavailable {
            unavailable: "node_missing".into(),
        },
        decisions: DetailDecisions {
            history_complete: false,
            adapter_used: None,
        },
        runs_since_transition: None,
        last_probe_at: None,
        probe_state: None,
    }
}

pub(super) fn decision_projection(
    decision: Option<&Decision>,
) -> codeterm_sentinel::graph_contracts::DecisionProjection {
    let decision = decision.filter(|decision| decision.valid());
    codeterm_sentinel::graph_contracts::DecisionProjection {
        decided_by: decision.map(|decision| decision.decided_by),
        adapter_id: decision
            .and_then(|decision| decision.adapter_id.as_deref())
            .map(|id| text(id, 256).0),
        model: decision
            .and_then(|decision| decision.model.as_deref())
            .map(|model| text(model, 256).0),
        confidence: decision.and_then(|decision| decision.confidence),
    }
}

#[cfg(test)]
pub(super) fn project_decision(result: &mut Value, decision: Option<&Decision>) {
    if let (Some(target), Value::Object(fields)) =
        (result.as_object_mut(), json!(decision_projection(decision)))
    {
        target.extend(fields);
    }
}

pub(crate) fn configured_fallback(
    adapter_id: Option<&str>,
    model: Option<&str>,
) -> Option<codeterm_sentinel::graph_contracts::ConfiguredFallback> {
    adapter_id.map(
        |id| codeterm_sentinel::graph_contracts::ConfiguredFallback {
            adapter_id: text(id, 256).0,
            model: model.map(|model| text(model, 256).0),
        },
    )
}

#[cfg(test)]
mod tests;
