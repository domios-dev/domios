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

pub(super) fn questions(snapshot: &Snapshot) -> DecisionQuestionsRequest {
    let mut questions = BTreeMap::from([("tick".into(), choice(
        "Classify whether the plan snapshot requires an Orchestrator decision. Hard wakes are enforced by the host. Deliberate review holds require no drift action. Use other for uncertainty.".into(),
        &[("action_needed", "A concrete action is needed"), ("status_only", "Healthy progress; no decision needed"), ("nothing_changed", "No meaningful change"), ("other", "Cannot classify confidently")],
    ))]);
    for worker in &snapshot.workers {
        questions.insert(format!("worker:{}:{}", worker.task_id, worker.tab_id), choice(
            format!("Classify task {} on tab {} using only its own bound-session evidence. Holds excuse review inactivity, never a missing tab/session or prompt. Use other for missing evidence.", worker.task_id, worker.tab_id),
            &[("on_track", "Healthy progress"), ("waiting_on_prompt", "A classified prompt needs an answer"), ("idle_no_report", "Idle beyond threshold with no report"), ("looping", "Turns repeat without task progress"), ("other", "Cannot classify confidently")],
        ));
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
    DecisionQuestionsRequest {
        state: json!({ "plan_id": snapshot.plan_id, "delta": snapshot.delta, "context": snapshot.context,
            "workers": snapshot.workers, "hard": snapshot.hard, "learnings": snapshot.learnings, "snapshot_hash": snapshot.hash() }),
        questions,
    }
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

pub(super) fn classify(
    snapshot: &Snapshot,
    model: &dyn DecisionModel,
    deadline: Instant,
) -> Result<Decision, DecisionError> {
    let request = questions(snapshot);
    let response = model.decide_questions(&request, deadline)?;
    request.validate_answers(&response.answers)?;
    let choices = choices(response)?;
    let uncertain = choices
        .values()
        .any(|(label, p)| label == "other" || !p.is_finite() || *p < MIN_CONFIDENCE);
    let mut decision = fallback(snapshot, "adapter", "model_answer");
    decision.learning_ids = Some(selected_learnings(&choices));
    decision.tick_label = choices["tick"].0.clone();
    decision.wake = !snapshot.hard.is_empty()
        || uncertain
        || choices["tick"].0 == "action_needed"
        || snapshot.delta.keys().any(|key| {
            key.starts_with("report:") || key.starts_with("task:") || key.starts_with("gate:")
        });
    decision.next_steps = snapshot
        .hard
        .iter()
        .map(|hard| hard.next_step(&snapshot.plan_id))
        .collect();
    decision.worker_labels.clear();
    for worker in &snapshot.workers {
        let label = &choices[&format!("worker:{}:{}", worker.task_id, worker.tab_id)].0;
        decision.worker_labels.insert(
            worker.tab_id.clone(),
            if label == "on_track" && worker.unhealthy() {
                worker.heuristic_label().into()
            } else {
                label.clone()
            },
        );
        let drift = matches!(label.as_str(), "idle_no_report" | "looping") && !worker.held;
        if drift || label == "waiting_on_prompt" || label == "other" {
            decision.wake = true;
            decision.next_steps.push(worker.next_step(label));
        }
        // Authoritative prompt and health evidence cannot be erased by a healthy model label.
        if worker.unhealthy() {
            decision.wake = true;
            if label == "on_track" {
                decision
                    .next_steps
                    .push(worker.next_step(worker.heuristic_label()));
            }
        }
    }
    if uncertain {
        decision.reason = "low_confidence";
    }
    if decision.wake && decision.next_steps.is_empty() {
        decision.next_steps.push(format!(
            "inspect the new plan evidence with codeterm plan state {} and choose its next action",
            snapshot.plan_id
        ));
    }
    Ok(decision)
}

pub(super) fn fallback(snapshot: &Snapshot, decided_by: &str, reason: &'static str) -> Decision {
    let mut next_steps: Vec<_> = snapshot
        .hard
        .iter()
        .map(|hard| hard.next_step(&snapshot.plan_id))
        .collect();
    let mut worker_labels = BTreeMap::new();
    for worker in &snapshot.workers {
        let label = worker.heuristic_label();
        worker_labels.insert(worker.tab_id.clone(), label.to_string());
        if worker.unhealthy() {
            next_steps.push(worker.next_step(label));
        }
    }
    let wake = !snapshot.delta.is_empty() || !next_steps.is_empty();
    if wake && next_steps.is_empty() {
        next_steps.push(format!(
            "review changed reports, strides and task states with codeterm plan state {}",
            snapshot.plan_id
        ));
    }
    Decision {
        wake,
        safety: false,
        tick_label: if wake {
            "action_needed"
        } else {
            "nothing_changed"
        }
        .into(),
        decided_by: decided_by.into(),
        reason,
        observation_id: crate::plan::ids::short_id("hb"),
        snapshot_hash: snapshot.hash(),
        next_steps,
        worker_labels,
        learning_ids: None,
    }
}
