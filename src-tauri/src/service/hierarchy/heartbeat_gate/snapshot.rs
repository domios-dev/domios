use super::*;
use crate::agent::chat_types::ChatMsg;
use crate::multiplexer::types::{Tab, TabBacking};
use crate::plan::{Plan, Task, TaskExecutionAttempt, TaskStatus};
use crate::service::ServiceContext;
use codeterm_plan::service_plan::state::task_projection;

fn encoded(value: &impl Serialize) -> String {
    crate::service::supporter::criteria_hash(&serde_json::to_vec(value).unwrap_or_default())
}

pub(super) fn plan_projection(plan: &Plan) -> BTreeMap<String, String> {
    let mut out = BTreeMap::from([("mode".into(), format!("{:?}:{:?}", plan.mode, plan.status))]);
    for task in &plan.tasks {
        project_task(plan, task, &mut out);
    }
    for stride in &plan.strides {
        out.insert(format!("stride:{}", stride.id), encoded(stride));
    }
    for gate in &plan.gates {
        out.insert(format!("gate:{}", gate.id), encoded(gate));
    }
    out
}

pub(super) fn task_wake(task_id: &str, status: &TaskStatus) -> HardWake {
    match status {
        TaskStatus::Done | TaskStatus::Cancelled => HardWake::TaskClosed(task_id.into()),
        TaskStatus::Blocked => HardWake::FailedGate(format!("task_blocked:{task_id}")),
        _ => HardWake::TaskState(task_id.into()),
    }
}

fn project_task(plan: &Plan, task: &Task, out: &mut BTreeMap<String, String>) {
    out.insert(
        format!("task:{}", task.id),
        encoded(&(
            &task.status,
            &task.status_label,
            &task.pause_reason,
            &task.interruption,
            &task.review_stage,
            &task.qa_state,
            &task.assigned_tab_id,
            &task_projection(plan, task).dependency_waits,
        )),
    );
    out.insert(
        format!("review:{}", task.id),
        encoded(&task.review_evidence),
    );
    for attempt in &task.execution_attempts {
        out.insert(
            format!("attempt:{}", attempt.id),
            encoded(&attempt.interruption),
        );
        if let Some(report) = &attempt.report {
            out.insert(format!("report:{}", report.id), encoded(report));
        }
    }
    for milestone in &task.milestones {
        out.insert(format!("stride:{}", milestone.id), encoded(milestone));
    }
}

fn projection_delta(
    projection: &BTreeMap<String, String>,
    baseline: &BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut delta: BTreeMap<_, _> = projection
        .iter()
        .filter(|(k, v)| baseline.get(*k) != Some(*v))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    for key in baseline.keys().filter(|k| !projection.contains_key(*k)) {
        delta.insert(key.clone(), "removed".into());
    }
    delta
}

/// Plan-derived observation for a Worker reminder: its scoped task and attempt
/// only, never transcripts, screens or lane processes.
pub(crate) fn collect_worker_scope(
    plan: &Plan,
    scope: &Scope,
    previous: &Observation,
) -> (Snapshot, Observation) {
    let task = plan
        .tasks
        .iter()
        .find(|task| scope.task_id.as_deref() == Some(task.id.as_str()));
    let mut projection = BTreeMap::new();
    let mut context = BTreeMap::new();
    if let Some(task) = task {
        project_task(plan, task, &mut projection);
        let attempt = task
            .execution_attempts
            .iter()
            .find(|attempt| scope.attempt_id.as_deref() == Some(attempt.id.as_str()));
        let mut row = task_context(plan, task);
        row["acknowledged"] = json!(attempt.is_some_and(|a| a.acknowledged_at.is_some()));
        row["reported"] = json!(attempt.is_some_and(|a| a.report.is_some()));
        context.insert(format!("task:{}", task.id), row);
    }
    let delta = projection_delta(
        &projection,
        previous.projection.as_ref().unwrap_or(&projection),
    );
    let snapshot = Snapshot {
        plan_id: plan.id.clone(),
        projection: projection.clone(),
        delta,
        context,
        ..Default::default()
    };
    let observed = Observation {
        projection: Some(projection),
        ..previous.clone()
    };
    (snapshot, observed)
}

pub(crate) fn collect_owner_messages(
    svc: &ServiceContext,
    tab_id: &str,
    previous: &Observation,
) -> (Vec<HardWake>, Option<Worker>) {
    let owner = { svc.mux.read().get_tab(tab_id).cloned() };
    let Some(owner) = owner else {
        return (Vec::new(), None);
    };
    let old = previous.workers.get(tab_id);
    let (turns, cursor, _) = own_session_turns(svc, &owner, old);
    let hard = turns
        .iter()
        .filter(|turn| is_owner_turn(turn))
        .map(|turn| HardWake::OwnerMessage(turn.id.clone()))
        .collect();
    let observed = Worker {
        session_key: session_key(&owner),
        turn_cursor: cursor,
        ..Default::default()
    };
    (hard, Some(observed))
}

/// Digest of the tab's own confirmed binding, narrowed to the attempt's
/// recorded session when given; the raw session key never leaves this module.
pub(crate) fn binding_digest(tab: &Tab, attempt: Option<&TaskExecutionAttempt>) -> Option<String> {
    match attempt {
        Some(attempt) => bound_attempt_session_key(tab, attempt),
        None => session_key(tab),
    }
    .map(|key| crate::service::supporter::criteria_hash(key.as_bytes()))
}

pub(crate) fn collect(
    svc: &ServiceContext,
    plan: &Plan,
    tab_id: &str,
    previous: &Observation,
    now: u64,
) -> (Snapshot, Observation) {
    let projection = plan_projection(plan);
    let delta = projection_delta(
        &projection,
        previous.projection.as_ref().unwrap_or(&projection),
    );
    let mut snapshot = Snapshot {
        plan_id: plan.id.clone(),
        projection: projection.clone(),
        delta,
        learnings: plan
            .heartbeat
            .learnings
            .iter()
            .rev()
            .filter(|l| !l.text.trim().is_empty())
            .take(12)
            .map(|l| (l.id.clone(), scrubbed(&l.text)))
            .collect(),
        ..Default::default()
    };
    (snapshot.context, snapshot.truncated) = delta_context(plan, &snapshot.delta);
    snapshot.hard = previous.pending_hard.clone();
    if snapshot.delta.contains_key("mode") {
        snapshot.hard.push(HardWake::ModeChange);
    }
    snapshot.hard.extend(
        svc.report_store
            .lock()
            .pending_for(tab_id)
            .iter()
            .map(|r| HardWake::UndeliveredReport(r.id.clone())),
    );
    for gate in &plan.gates {
        if gate.status == crate::plan::GateStatus::Escalated {
            snapshot.hard.push(HardWake::FailedGate(gate.id.clone()));
        }
        if snapshot.delta.contains_key(&format!("gate:{}", gate.id)) && gate.answer.is_some() {
            snapshot.hard.push(HardWake::ChoiceAnswer(gate.id.clone()));
        }
    }
    let mut observed = previous.clone();
    observed.projection = Some(projection);
    let choices = {
        let notifications = svc.notif.lock();
        notifications
            .list(false)
            .into_iter()
            .filter(|n| {
                n.answer.as_ref().is_some_and(|a| {
                    a.asked_from_tab.as_deref() == Some(tab_id) && a.answered.is_some()
                })
            })
            .map(|n| n.id.clone())
            .collect::<Vec<_>>()
    };
    for id in choices
        .iter()
        .filter(|id| !previous.answered_choices.contains(id))
    {
        snapshot.hard.push(HardWake::ChoiceAnswer(id.clone()));
    }
    observed.answered_choices = choices;
    for task in &plan.tasks {
        if snapshot.delta.contains_key(&format!("task:{}", task.id)) {
            snapshot.hard.push(task_wake(&task.id, &task.status));
        }
        if snapshot.delta.contains_key(&format!("review:{}", task.id)) {
            if let Some(review) = task
                .review_evidence
                .last()
                .filter(|e| e.verdict == crate::plan::ReviewVerdict::Bounce)
            {
                snapshot.hard.push(HardWake::FailedGate(review.id.clone()));
            }
        }
        for attempt in &task.execution_attempts {
            if attempt.interruption.reason.is_some()
                && (attempt.closed_at.is_none()
                    || snapshot
                        .delta
                        .contains_key(&format!("attempt:{}", attempt.id)))
            {
                snapshot.hard.push(HardWake::Incident(format!(
                    "dispatch_or_worker_failure:{}",
                    attempt.id
                )));
            }
            if let Some(report) = &attempt.report {
                if snapshot
                    .delta
                    .contains_key(&format!("report:{}", report.id))
                {
                    snapshot.hard.push(HardWake::NewReport(report.id.clone()));
                }
            }
        }
        if !task.status.counts_as_open_work() {
            continue;
        }
        if task.status == TaskStatus::Pending
            && task.assigned_tab_id.is_none()
            && crate::service::plan::state::task_projection(plan, task).needs_orchestrator_action()
        {
            snapshot
                .hard
                .push(HardWake::Incident(format!("dispatch_required:{}", task.id)));
        }
        for attempt in &task.execution_attempts {
            if attempt.closed_at.is_some() {
                continue;
            }
            let worker = collect_worker(svc, task, attempt, previous, now);
            if !worker.ack_received {
                snapshot
                    .hard
                    .push(HardWake::Incident(format!("worker_ack:{}", attempt.id)));
            }
            if worker.session_key.is_none() || worker.process != "present" {
                snapshot.hard.push(HardWake::Incident(format!(
                    "worker_health:{}",
                    worker.tab_id
                )));
            }
            let old = previous.workers.get(&worker.tab_id);
            let changed = old.is_some_and(|old| {
                old.session_key != worker.session_key
                    || old.last_turn != worker.last_turn
                    || old.lane_commits != worker.lane_commits
            });
            let last = if changed || worker.activity == "working" {
                now
            } else {
                previous
                    .last_activity
                    .get(&worker.tab_id)
                    .copied()
                    .unwrap_or_else(|| {
                        if task.status_changed_at > 0 {
                            (task.status_changed_at / 1000).min(now)
                        } else {
                            now
                        }
                    })
            };
            observed.last_activity.insert(worker.tab_id.clone(), last);
            let mut worker = worker;
            worker.idle_overdue =
                !worker.reported && now.saturating_sub(last) >= IDLE_NO_REPORT_SECS;
            observed
                .workers
                .insert(worker.tab_id.clone(), worker.clone());
            snapshot.workers.push(worker);
        }
        if task.status == TaskStatus::InProgress
            && task
                .execution_attempts
                .iter()
                .all(|a| a.closed_at.is_some())
        {
            snapshot
                .hard
                .push(HardWake::Incident(format!("dispatch_missing:{}", task.id)));
        }
        if task.interruption.handoff_required {
            snapshot
                .hard
                .push(HardWake::Incident(format!("task_interruption:{}", task.id)));
        }
    }
    let (owner_messages, owner_observation) = collect_owner_messages(svc, tab_id, previous);
    snapshot.hard.extend(owner_messages);
    if let Some(owner_observation) = owner_observation {
        observed.workers.insert(tab_id.into(), owner_observation);
    }
    let mut unique_hard = Vec::new();
    for hard in snapshot.hard.drain(..) {
        if !unique_hard.contains(&hard) {
            unique_hard.push(hard);
        }
    }
    snapshot.hard = unique_hard;
    observed.pending_hard = snapshot.hard.clone();
    snapshot
        .workers
        .sort_by(|a, b| (&a.task_id, &a.tab_id).cmp(&(&b.task_id, &b.tab_id)));
    (snapshot, observed)
}

pub(super) fn session_key(tab: &Tab) -> Option<String> {
    let own_backend = match &tab.backing {
        TabBacking::ChatBackend { session_id, .. } => {
            tab.agent_session_id.as_deref() == Some(session_id)
        }
        TabBacking::Pty => true,
        TabBacking::Surface(_) => false,
    };
    (own_backend && crate::service::plan_reconcile::tab_has_live_agent(tab))
        .then(|| {
            Some(format!(
                "{}:{}",
                tab.agent_session_id.as_deref()?,
                tab.session_binding_nonce.as_deref()?
            ))
        })
        .flatten()
}

pub(super) fn bound_attempt_session_key(
    tab: &Tab,
    attempt: &TaskExecutionAttempt,
) -> Option<String> {
    session_key(tab).filter(|_| {
        attempt
            .session_id
            .as_deref()
            .is_none_or(|sid| tab.agent_session_id.as_deref() == Some(sid))
            && attempt
                .session_binding_nonce
                .as_deref()
                .is_none_or(|nonce| tab.session_binding_nonce.as_deref() == Some(nonce))
    })
}

fn collect_worker(
    svc: &ServiceContext,
    task: &Task,
    attempt: &TaskExecutionAttempt,
    previous: &Observation,
    now: u64,
) -> Worker {
    let tab_id = &attempt.executor_tab_id;
    let tab = { svc.mux.read().get_tab(tab_id).cloned() };
    let mut worker = Worker {
        task_id: task.id.clone(),
        tab_id: tab_id.clone(),
        ack_received: attempt.acknowledged_at.is_some(),
        reported: attempt.report.is_some(),
        held: crate::plan::reconcile::active_review_hold_note(task).is_some(),
        elapsed_bucket: now.saturating_sub(task.status_changed_at / 1000) / IDLE_NO_REPORT_SECS,
        process: "absent".into(),
        ..Default::default()
    };
    let Some(tab) = tab else {
        return worker;
    };
    worker.session_key = bound_attempt_session_key(&tab, attempt);
    worker.activity = match svc.resolved_tab_status(&tab) {
        Some(codeterm_types::pty::TabStatus::Working) => "working",
        Some(codeterm_types::pty::TabStatus::Idle) => "idle",
        Some(codeterm_types::pty::TabStatus::Waiting) => "waiting",
        Some(codeterm_types::pty::TabStatus::Error) => "error",
        Some(codeterm_types::pty::TabStatus::Dead) => "dead",
        None => "unknown",
    }
    .into();
    worker.prompt = tab.pending_prompt_kind.as_ref().map(|kind| {
        serde_json::to_value(kind)
            .unwrap_or_default()
            .as_str()
            .unwrap_or("unknown")
            .to_string()
    });
    let safe_choice = tab
        .pending_prompt
        .as_ref()
        .filter(|_| tab.pending_prompt_kind == Some(codeterm_types::pty::PromptKind::ManualUpdate))
        .and_then(|prompt| {
            svc.agent_registry
                .with_provider(tab.agent_type.as_deref()?, |p| {
                    p.update_prompt_safe_choice(prompt)
                })
                .flatten()
        });
    worker.safe_option = tab.pending_prompt.as_ref().and_then(|prompt| {
        prompt
            .options
            .iter()
            .position(|option| Some(&option.value) == safe_choice.as_ref())
            .map(|index| index + 1)
    });
    worker.safe_choice = tab.pending_prompt.as_ref().and_then(|prompt| {
        prompt
            .options
            .iter()
            .find(|option| Some(&option.value) == safe_choice.as_ref())
            .map(|option| option.label.clone())
    });
    worker.process = match &tab.backing {
        TabBacking::ChatBackend {
            plugin_id,
            session_id,
        } => {
            if svc.chatbackend_registry.get(plugin_id).is_some()
                && tab.agent_session_id.as_deref() == Some(session_id)
            {
                "present"
            } else {
                "unknown"
            }
        }
        TabBacking::Pty => tab
            .pty_id
            .as_deref()
            .and_then(|pty| svc.observe_provider_process_tree(tab_id, pty))
            .map(|tree| match tree.state() {
                crate::service::process_tree::ProviderTreeState::Present => "present",
                crate::service::process_tree::ProviderTreeState::Unknown => "unknown",
                _ => "absent",
            })
            .unwrap_or("unknown"),
        TabBacking::Surface(_) => "absent",
    }
    .into();
    if worker.session_key.is_none() {
        worker.evidence_unavailable = true;
        return worker;
    }
    let old = previous.workers.get(tab_id);
    let (turns, cursor, unavailable) = own_session_turns(svc, &tab, old);
    worker.turn_cursor = cursor;
    worker.evidence_unavailable = unavailable;
    worker.appended_turns = if old.is_some() {
        turns.iter().map(|m| m.id.clone()).collect()
    } else {
        Vec::new()
    };
    worker.last_turn = turns
        .last()
        .map(|m| m.id.clone())
        .or_else(|| old.and_then(|w| w.last_turn.clone()));
    worker.recent_turn_hashes =
        recent_turn_hashes(old.filter(|w| w.session_key == worker.session_key), &turns);
    worker.repeating = worker.recent_turn_hashes.len() == 3
        && worker
            .recent_turn_hashes
            .iter()
            .all(|h| Some(h) == worker.recent_turn_hashes.last());
    worker.lane_commits = lane_commits(task, attempt).unwrap_or_else(|_| {
        worker.evidence_unavailable = true;
        Vec::new()
    });
    if old.is_some_and(|old| old.lane_commits != worker.lane_commits) {
        worker.recent_turn_hashes.clear();
        worker.repeating = false;
    }
    worker
}

pub(super) fn recent_turn_hashes(previous: Option<&Worker>, turns: &[ChatMsg]) -> Vec<String> {
    let mut hashes = previous
        .map(|w| w.recent_turn_hashes.clone())
        .unwrap_or_default();
    hashes.extend(
        turns
            .iter()
            .filter(|m| m.kind == "assistant" && !m.content.trim().is_empty())
            .map(|m| encoded(&m.content)),
    );
    hashes
        .into_iter()
        .rev()
        .take(3)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect()
}

fn own_session_turns(
    svc: &ServiceContext,
    tab: &Tab,
    previous: Option<&Worker>,
) -> (Vec<ChatMsg>, Option<String>, bool) {
    if session_key(tab).is_none() {
        return (Vec::new(), None, true);
    }
    let previous = previous.filter(|w| w.session_key == session_key(tab));
    let cursor = previous.and_then(|w| w.turn_cursor.as_deref());
    match &tab.backing {
        TabBacking::ChatBackend {
            plugin_id,
            session_id,
        } => {
            let path =
                crate::service::tab_chat::chatbackend_history::session_path(plugin_id, session_id);
            match bounded_host_history(&path) {
                Ok(messages) => {
                    let last = messages.last().map(|m| m.id.clone());
                    let start = cursor
                        .and_then(|id| messages.iter().position(|m| m.id == id))
                        .map(|i| i + 1)
                        .unwrap_or(0);
                    (
                        messages.into_iter().skip(start).collect(),
                        last.or_else(|| cursor.map(str::to_string)),
                        false,
                    )
                }
                Err(_) => (Vec::new(), cursor.map(str::to_string), true),
            }
        }
        TabBacking::Surface(_) => (Vec::new(), None, true),
        TabBacking::Pty => {
            let Some(provider) = tab.agent_type.as_deref() else {
                return (Vec::new(), None, true);
            };
            let sid = tab.agent_session_id.as_deref().unwrap_or_default();
            let cwd = tab.cwd.as_deref().unwrap_or_default();
            svc.agent_registry
                .with_provider(provider, |p| {
                    if p.uses_structured_chat() {
                        let delta = p.read_structured_chat(cwd, sid, cursor);
                        let unavailable = delta.cursor.is_none();
                        return (delta.messages, delta.cursor, unavailable);
                    }
                    let Some(path) = p.session_file_path(cwd, sid) else {
                        return (Vec::new(), None, true);
                    };
                    let Ok(meta) = std::fs::metadata(&path) else {
                        return (Vec::new(), None, true);
                    };
                    let from = cursor
                        .and_then(|s| s.parse::<u64>().ok())
                        .filter(|n| *n <= meta.len())
                        .unwrap_or_else(|| {
                            codeterm_plugin::jsonl_window::tail_window_start(
                                &path,
                                meta.len(),
                                codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES,
                            )
                        });
                    let delta = p.parse_session_delta(&path, from);
                    let messages =
                        crate::service::tab_chat::rich_rows_to_chat_msgs(&delta.messages, from);
                    (messages, Some(delta.new_offset.to_string()), false)
                })
                .unwrap_or((Vec::new(), None, true))
        }
    }
}

fn lane_commits(task: &Task, attempt: &TaskExecutionAttempt) -> Result<Vec<String>, String> {
    let (Some(path), Some(branch)) = (
        attempt
            .worktree_path
            .as_ref()
            .or(task.lane.worktree_path.as_ref()),
        attempt.branch.as_ref().or(task.lane.branch.as_ref()),
    ) else {
        return Ok(Vec::new());
    };
    let mut command = std::process::Command::new("git");
    crate::process_util::no_window_std(&mut command);
    let output = command
        .args([
            "-C",
            path,
            "--no-pager",
            "log",
            "-8",
            "--format=%H",
            "--end-of-options",
            branch,
            "--",
        ])
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .map_err(|e| e.to_string())?;
    if !output.status.success() {
        return Err("lane_git_unavailable".into());
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::to_string)
        .collect())
}

fn bounded_host_history(path: &std::path::Path) -> std::io::Result<Vec<ChatMsg>> {
    use std::io::{Read, Seek, SeekFrom};
    let mut file = std::fs::File::open(path)?;
    let len = file.metadata()?.len();
    let from = len.saturating_sub(codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES);
    file.seek(SeekFrom::Start(from))?;
    let mut bytes = Vec::new();
    file.take(codeterm_plugin::jsonl_window::DELTA_CHUNK_BYTES)
        .read_to_end(&mut bytes)?;
    let start = if from == 0 {
        0
    } else {
        bytes
            .iter()
            .position(|b| *b == b'\n')
            .map(|i| i + 1)
            .unwrap_or(bytes.len())
    };
    Ok(bytes[start..]
        .split(|b| *b == b'\n')
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect())
}

pub(super) fn is_owner_turn(message: &ChatMsg) -> bool {
    if message.kind != "user" || message.system_subtype.is_some() {
        return false;
    }
    let marker = codeterm_types::message_markers::parse_host_injected(&message.content);
    let kind = message.inbound_kind.as_deref().or_else(|| {
        marker.as_ref().map(|m| {
            if m.kind == "inbound" {
                m.attr("kind").unwrap_or("unknown")
            } else {
                &m.kind
            }
        })
    });
    (kind == Some("telegram") || message.from_tab.is_none())
        && !matches!(
            kind,
            Some("system" | "system_prompt" | "report" | "tab" | "reminder" | "plugin" | "pebble")
        )
}

fn delta_context(plan: &Plan, delta: &BTreeMap<String, String>) -> (BTreeMap<String, Value>, bool) {
    let mut context = BTreeMap::new();
    if delta.contains_key("mode") {
        context.insert(
            "mode".into(),
            json!({"mode": plan.mode, "status": plan.status}),
        );
    }
    for task in &plan.tasks {
        let key = format!("task:{}", task.id);
        if delta.contains_key(&key) {
            context.insert(key, task_context(plan, task));
        }
        for attempt in &task.execution_attempts {
            if let Some(report) = &attempt.report {
                let key = format!("report:{}", report.id);
                if delta.contains_key(&key) {
                    context.insert(
                        key,
                        json!({"task": task.id, "status": report.status, "sha": report.sha,
                        "test_result": report.test_result.as_deref().map(scrubbed)}),
                    );
                }
            }
        }
    }
    for stride in plan
        .strides
        .iter()
        .chain(plan.tasks.iter().flat_map(|t| &t.milestones))
    {
        let key = format!("stride:{}", stride.id);
        if delta.contains_key(&key) {
            context.insert(
                key,
                json!({"kind": stride.kind, "text": scrubbed(&stride.text)}),
            );
        }
    }
    for gate in &plan.gates {
        let key = format!("gate:{}", gate.id);
        if delta.contains_key(&key) {
            context.insert(key, json!({"status": gate.status, "answer": gate.answer.as_ref().map(|a| scrubbed(&a.text))}));
        }
    }
    let truncated = context.len() > MAX_OBSERVATION_ROWS;
    (
        context.into_iter().take(MAX_OBSERVATION_ROWS).collect(),
        truncated,
    )
}

fn task_context(plan: &Plan, task: &Task) -> Value {
    let projection = task_projection(plan, task);
    json!({"task": task.id, "status": task.status,
        "status_label": task.status_label.as_deref().map(scrubbed),
        "pause_reason": task.pause_reason.as_deref().map(scrubbed),
        "assigned_tab_id": task.assigned_tab_id,
        "presentation": projection.presentation,
        "dependency_waits": projection.dependency_waits.iter().map(|wait| json!({
            "task_id": wait.task_id, "status": wait.status, "missing": wait.missing
        })).collect::<Vec<_>>(),
        "held": crate::plan::reconcile::active_review_hold_note(task).is_some()})
}

fn scrubbed(text: &str) -> String {
    crate::service::launch_diagnostics::redact_secrets(text)
        .chars()
        .take(512)
        .collect()
}
