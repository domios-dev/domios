use super::*;
use codeterm_types::decision::{DecisionQuestion, NamedDecisionQuestion};

fn choice(instructions: String, options: &[(&str, &str)]) -> NamedDecisionQuestion {
    NamedDecisionQuestion {
        instructions,
        question: DecisionQuestion::Choice {
            options: options
                .iter()
                .map(|(k, v)| (k.to_string(), v.to_string()))
                .collect(),
        },
    }
}

fn tick_question() -> NamedDecisionQuestion {
    choice(
        "Decide whether the host-authored reminder named in state.reminder would help its recipient act now. Choose action_needed only when the plan state shows a concrete step for that recipient. Choose blocked when the recipient cannot act while waiting on the owner, a dependency or an external blocker. Healthy progress, deliberate review holds and unchanged state need no reminder. Hard events are evidence, not an obligation. Use other for uncertainty.".into(),
        &[("action_needed", "The reminder is useful now"), ("blocked", "The recipient cannot act now"), ("status_only", "Healthy progress; no reminder needed"), ("nothing_changed", "No meaningful change"), ("other", "Cannot classify confidently")],
    )
}

fn worker_question(task: &str, tab: &str) -> NamedDecisionQuestion {
    choice(
        format!("Classify task {task} on tab {tab} using only its own health evidence. Holds excuse review inactivity, never a missing tab/session or prompt. Use other for missing evidence."),
        &[("on_track", "Healthy progress"), ("waiting_on_prompt", "A classified prompt needs an answer"), ("idle_no_report", "Idle beyond threshold with no report"), ("looping", "Turns repeat without task progress"), ("other", "Cannot classify confidently")],
    )
}

/// Changes whenever the approval questions or threshold change, so a revised
/// criterion is a new observation rather than a cached verdict.
pub(crate) fn criteria_version() -> String {
    let criteria = json!({ "tick": tick_question(), "worker": worker_question("{task}", "{tab}"),
        "min_confidence": MIN_CONFIDENCE });
    crate::service::supporter::criteria_hash(&serde_json::to_vec(&criteria).unwrap_or_default())
}

pub(super) fn questions(snapshot: &Snapshot) -> DecisionQuestionsRequest {
    let mut questions = BTreeMap::from([("tick".into(), tick_question())]);
    for worker in &snapshot.workers {
        questions.insert(
            format!("worker:{}:{}", worker.task_id, worker.tab_id),
            worker_question(&worker.task_id, &worker.tab_id),
        );
    }
    for id in snapshot.learnings.keys() {
        questions.insert(
            format!("learning:{id}"),
            choice(
                format!(
                    "Is learning {id} relevant to the current snapshot's concrete next action?"
                ),
                &[
                    ("relevant", "Directly helps the current action"),
                    ("irrelevant", "Does not help this situation"),
                    ("other", "Uncertain"),
                ],
            ),
        );
    }
    let workers: Vec<_> = snapshot.workers.iter().map(Worker::health).collect();
    DecisionQuestionsRequest {
        state: json!({ "reminder": { "kind": snapshot.scope.kind, "role": snapshot.scope.role,
                "task_id": snapshot.scope.task_id },
            "plan_id": snapshot.plan_id, "changed": snapshot.delta.keys().collect::<Vec<_>>(),
            "context": snapshot.context, "workers": workers, "hard": snapshot.hard,
            "learnings": snapshot.learnings, "snapshot_hash": snapshot.hash() }),
        questions,
    }
}

/// None when the observation would exceed its byte or row bounds; a partial
/// authority context is suppressed, never approved.
pub(super) fn bounded_questions(snapshot: &Snapshot) -> Option<DecisionQuestionsRequest> {
    if snapshot.truncated
        || snapshot.workers.len() > MAX_OBSERVATION_ROWS
        || snapshot.hard.len() > MAX_OBSERVATION_ROWS
        || snapshot.context.len() > MAX_OBSERVATION_ROWS
    {
        return None;
    }
    let request = questions(snapshot);
    (serde_json::to_vec(&request).ok()?.len() <= MAX_OBSERVATION_BYTES).then_some(request)
}

pub(super) fn selected_learnings(choices: &BTreeMap<String, (String, f64)>) -> Vec<String> {
    choices
        .iter()
        .filter_map(|(name, (label, confidence))| {
            (label == "relevant" && *confidence >= MIN_CONFIDENCE)
                .then(|| name.strip_prefix("learning:").map(str::to_string))
                .flatten()
        })
        .take(3)
        .collect()
}

fn choices(response: DecisionResponse) -> Result<BTreeMap<String, (String, f64)>, DecisionError> {
    response
        .answers
        .into_iter()
        .map(|(name, answer)| {
            let DecisionAnswer::Choice {
                choice,
                confidence,
                probabilities,
            } = answer
            else {
                return Err(DecisionError::Parse("invalid_answer".into()));
            };
            let confidence = confidence
                .or_else(|| probabilities.get(&choice).copied())
                .unwrap_or(0.0);
            Ok((name, (choice, confidence)))
        })
        .collect()
}

/// Wakes only on a validated `action_needed` whose every answer is a declared
/// choice at finite confidence of at least MIN_CONFIDENCE.
pub(super) fn classify(
    snapshot: &Snapshot,
    request: DecisionQuestionsRequest,
    model: &dyn DecisionModel,
    deadline: Instant,
) -> Result<Decision, DecisionError> {
    let response = model.decide_questions(&request, deadline)?;
    request.validate_answers(&response.answers)?;
    let choices = choices(response)?;
    let uncertain = choices
        .values()
        .any(|(label, p)| label == "other" || !p.is_finite() || *p < MIN_CONFIDENCE);
    let reason = if uncertain {
        "low_confidence"
    } else {
        "model_answer"
    };
    let mut decision = suppressed(snapshot, "adapter", reason);
    decision.learning_ids = Some(selected_learnings(&choices));
    decision.tick_label = choices["tick"].0.clone();
    decision.wake = !uncertain && decision.tick_label == "action_needed";
    decision.worker_labels.clear();
    for worker in &snapshot.workers {
        let label = &choices[&format!("worker:{}:{}", worker.task_id, worker.tab_id)].0;
        let label = if label == "on_track" && worker.unhealthy() {
            worker.heuristic_label().to_string()
        } else {
            label.clone()
        };
        let drift = matches!(label.as_str(), "idle_no_report" | "looping") && !worker.held;
        if decision.wake && (drift || worker.unhealthy() || label == "waiting_on_prompt") {
            decision.next_steps.push(worker.next_step(&label));
        }
        decision.worker_labels.insert(worker.tab_id.clone(), label);
    }
    if decision.wake {
        let mut steps: Vec<String> = snapshot
            .hard
            .iter()
            .map(|hard| hard.next_step(&snapshot.plan_id))
            .collect();
        steps.append(&mut decision.next_steps);
        if steps.is_empty() {
            steps.push(format!(
                "inspect the new plan evidence with codeterm plan state {} and choose its next action",
                snapshot.plan_id
            ));
        }
        decision.next_steps = steps;
    }
    Ok(decision)
}

pub(super) fn error_reason(error: &DecisionError) -> &'static str {
    match error {
        DecisionError::Budget => "budget_exhausted",
        DecisionError::CreditExhausted => "credit_exhausted",
        DecisionError::Deadline => "model_deadline",
        DecisionError::Unavailable(_) => "adapter_disconnected",
        DecisionError::Parse(_) => "invalid_answer",
        _ => "model_failed",
    }
}

/// A decision that never wakes; heuristic worker labels are journal context only.
pub(super) fn suppressed(snapshot: &Snapshot, decided_by: &str, reason: &'static str) -> Decision {
    Decision {
        wake: false,
        tick_label: "nothing_changed".into(),
        decided_by: decided_by.into(),
        reason,
        observation_id: crate::plan::ids::short_id("hb"),
        snapshot_hash: snapshot.hash(),
        next_steps: Vec::new(),
        worker_labels: snapshot
            .workers
            .iter()
            .map(|worker| (worker.tab_id.clone(), worker.heuristic_label().to_string()))
            .collect(),
        learning_ids: None,
        scope: snapshot.scope.clone(),
        adapter: None,
        model: None,
    }
}
