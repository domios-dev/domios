use crate::client::SocketCallError;
use serde_json::Value;

pub(crate) fn render_plan_call_error(err: SocketCallError, params: &Value) -> String {
    match err {
        SocketCallError::Rpc(rpc) => {
            let rendered = rpc
                .data
                .as_ref()
                .and_then(|data| render_plan_error(data, params));
            match rendered {
                Some(message) => message,
                None => append_cross_daemon_hint(
                    format!("RPC error {}: {}", rpc.code, rpc.message),
                    params,
                    &rpc.message,
                ),
            }
        }
        SocketCallError::Transport(message) => message,
    }
}

pub(crate) fn render_plan_error(data: &Value, params: &Value) -> Option<String> {
    let kind = data.get("type")?.as_str()?;
    let plan_id = params
        .get("plan_id")
        .and_then(|v| v.as_str())
        .unwrap_or("<id>");
    let task_id = params.get("task_id").and_then(|v| v.as_str());
    let loop_id = params
        .get("loop_id")
        .and_then(|v| v.as_str())
        .unwrap_or("<loop_id>");
    match kind {
        "undeclared_kind" => render_undeclared_kind(data, plan_id, task_id),
        "bad_status" => Some(render_bad_status(plan_id, task_id, params)),
        "bad_mode" => Some(format!(
            "✗ invalid plan mode.\nvalid modes: planning|approved|execution|complete\nfix: codeterm plan mode {plan_id} --set planning\nor approve a ready plan with: codeterm plan approve {plan_id}"
        )),
        "bad_priority" => Some(format!(
            "✗ invalid task priority.\nvalid priorities: p0|p1|p2|p3\nfix: codeterm plan task update {plan_id} {} --priority p2",
            task_id.unwrap_or("<task_id>")
        )),
        "not_found" => Some(render_not_found(plan_id, loop_id, params)),
        "task_not_found" => Some(with_cross_daemon_hint(format!(
            "✗ no task '{}' was found in plan '{plan_id}'.\nfix: codeterm plan list --active",
            task_id.unwrap_or("<task_id>")
        ))),
        "element_not_found" => Some(format!(
            "✗ no element '{}' was found in plan '{plan_id}'.\nfix: codeterm plan element list {plan_id}",
            params
                .get("element_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<element_id>")
        )),
        "transition_denied" => render_transition_denied(data, plan_id, task_id),
        "open_subtasks" => render_open_subtasks(data, plan_id, task_id),
        "task_creation_requires_approval" => Some(format!(
            "✗ this plan is still in planning mode.\n\
finish planning before generating execution tasks:\n\
  codeterm docs ORCHESTRATION\n\
  codeterm plan state {plan_id}\n\
  codeterm plan doctor {plan_id} --kind quality\n\
  codeterm plan approve {plan_id}\n\
then retry the task add. If you intentionally need a draft task, pass `--force`."
        )),
        "heartbeat_requires_execution" => Some(format!(
            "✗ heartbeat is for active execution state, not planning notes.\n\
Use spec/plan sections for approval rules, or mem for durable preferences:\n\
  codeterm docs ORCHESTRATION\n\
  codeterm plan state {plan_id}\n\
  codeterm mem save-preference --scope global --title \"CodeTerm planner preference\" \"<preference>\"\n\
After approval/execution starts, set heartbeat with:\n\
  codeterm orchestrator heartbeat set --plan {plan_id} --focus \"<now>\" --lane \"<active-lane>\""
        )),
        "budget_exhausted" => {
            let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let budget = data.get("budget").and_then(|v| v.as_u64()).unwrap_or(0);
            Some(format!(
                "✗ loop budget {count}/{budget} reached for loop '{loop_id}'; extend with codeterm plan loop set {loop_id} --budget N, or close the plan."
            ))
        }
        "in_progress_requires_pane" => Some(format!(
            "✗ in_progress requires an executor pane.\nfix: set status and pane in one command:\n  codeterm plan task update {plan_id} {} --status in_progress --pane <pane-id>\n(emergency override: CODETERM_STATE_INVARIANTS=off)",
            task_id.unwrap_or("<task_id>")
        )),
        "blocked_requires_label" => Some(format!(
            "✗ blocked is a claim, not a parking spot.\nfix: say what unblocks it, or open a decision gate:\n  codeterm plan task update {plan_id} {} --status blocked --status-label \"<what unblocks it>\"\n  codeterm plan decision ask {plan_id} \"<question>\"",
            task_id.unwrap_or("<task_id>")
        )),
        "park_requires_reason" => Some(format!(
            "✗ park requires a reason.\nfix: codeterm plan task update {plan_id} {} --status parked --note \"<reason>\"",
            task_id.unwrap_or("<task_id>")
        )),
        "pause_requires_reason" => Some(format!(
            "✗ pause requires a reason.\nfix: codeterm plan pause {plan_id} --reason \"<why this plan is paused>\""
        )),
        "qa_note_required" => Some(format!(
            "✗ QA queue requires a note.\nfix: codeterm plan qa queue --plan {plan_id} --task {} --note \"<what to verify>\"",
            task_id.unwrap_or("<task_id>")
        )),
        "qa_slot_unavailable" => Some(format!(
            "✗ QA slot is taken — only one task runs QA at a time.\nfix: wait for the current QA run to finish or check the queue:\n  codeterm plan qa queue --plan {plan_id}"
        )),
        "qa_not_running" => Some(format!(
            "✗ no QA is running on this plan.\nfix: queue the task for QA first:\n  codeterm plan qa queue --plan {plan_id} --task {}",
            task_id.unwrap_or("<task_id>")
        )),
        "gate_not_found" => Some(format!(
            "✗ decision gate not found.\nfix: list open gates:\n  codeterm plan decision list {plan_id}"
        )),
        "gate_already_answered" => Some(format!(
            "✗ this decision gate was already answered.\nfix: list all gates to see the recorded answer:\n  codeterm plan decision list {plan_id}"
        )),
        "gate_requires_question" => Some(
            "✗ decision gate requires a question.\nfix: codeterm plan decision ask <plan_id> \"<question>\"".into()
        ),
        "group_not_found" => Some(format!(
            "✗ no such task group on plan '{plan_id}'.\nfix: declare the group first, then record the run:\n  codeterm plan groups set {plan_id} --groups \"<key:label>\"\n  codeterm plan qa next {plan_id} --group <key> --claim --note \"<scope>\""
        )),
        "waiting_focus_requires_open_gate" => Some(format!(
            "✗ waiting_focus status requires an open decision gate.\nfix: ask a gate question first, then set the focus:\n  codeterm plan decision ask {plan_id} \"<question>\"\n  codeterm orchestrator heartbeat set --plan {plan_id} --focus waiting_focus"
        )),
        "review_wip_exceeded" => {
            let wip_count = data.get("wip_count").and_then(|v| v.as_u64()).unwrap_or(0);
            let wip_limit = data.get("wip_limit").and_then(|v| v.as_u64()).unwrap_or(0);
            let credit = data.get("credit").and_then(|v| v.as_i64()).unwrap_or(0);
            Some(format!(
                "✗ review WIP exceeded: {wip_count}/{wip_limit} tasks in_review (credit={credit}).\nfix: close a review first, then retry dispatch:\n  codeterm plan review status {plan_id}\n  codeterm plan review accept {plan_id} <task_id> --note \"<verdict>\""
            ))
        }
        "review_evidence_required" => Some(format!(
            "✗ in_review requires evidence: an executable exact QA plan (acceptance_criteria + test_plan) or attached independent review evidence.\nfix: set both fields, or record review evidence first:\n  codeterm plan task update {plan_id} {} --status in_review",
            task_id.unwrap_or("<task_id>")
        )),
        "review_round_exceeded" => {
            let round = data.get("round").and_then(|v| v.as_u64()).unwrap_or(0);
            let max_rounds = data.get("max_rounds").and_then(|v| v.as_u64()).unwrap_or(0);
            Some(format!(
                "✗ review round {round} exceeds the default max of {max_rounds}.\nfix: record a P0 exception first:\n  codeterm plan review override {plan_id} {} --owner <you> --reason \"<why>\" --reproduction \"<repro steps>\"",
                task_id.unwrap_or("<task_id>")
            ))
        }
        "review_oracle_scope_expansion" => Some(format!(
            "✗ correction review cannot expand the fixed acceptance oracle without new reproducible release-impact evidence.\nfix: record a P0 override to justify the scope change:\n  codeterm plan review override {plan_id} {} --owner <you> --reason \"<why>\" --reproduction \"<repro steps>\"",
            task_id.unwrap_or("<task_id>")
        )),
        "review_override_incomplete" => Some(
            "✗ review override requires non-empty --owner, --reason, and --reproduction.".into(),
        ),
        "review_not_in_review" => Some(format!(
            "✗ task '{}' is not in_review.\nfix: codeterm plan task update {plan_id} {} --status in_review",
            task_id.unwrap_or("<task_id>"),
            task_id.unwrap_or("<task_id>")
        )),
        "write_pending" => render_write_pending(data, plan_id, task_id),
        "readback_mismatch" => render_readback_mismatch(data, plan_id, task_id),
        "io" => Some(render_io(data, plan_id, params)),
        "ref_not_found" => render_ref_not_found(data),
        "ref_ambiguous" => render_ref_ambiguous(data),
        _ => None,
    }
}

fn render_ref_not_found(data: &Value) -> Option<String> {
    let kind = data.get("kind")?.as_str()?;
    let reference = data.get("reference")?.as_str()?;
    Some(format!(
        "✗ no {kind} matches '{reference}'.\nfix: pass the full id or a longer unique prefix; list candidates with the matching list command."
    ))
}

fn render_ref_ambiguous(data: &Value) -> Option<String> {
    let kind = data.get("kind")?.as_str()?;
    let reference = data.get("reference")?.as_str()?;
    let candidates = data.get("candidates")?.as_array()?;
    let listed = candidates
        .iter()
        .filter_map(|c| {
            let id = c.get("id")?.as_str()?;
            let label = c.get("label").and_then(|v| v.as_str()).unwrap_or("");
            Some(format!("{id} ({label})"))
        })
        .collect::<Vec<_>>()
        .join("; ");
    Some(format!(
        "✗ {kind} ref '{reference}' is ambiguous — matches: {listed}\nfix: pass a longer prefix or the full id."
    ))
}

fn render_bad_status(plan_id: &str, task_id: Option<&str>, params: &Value) -> String {
    if is_task_status_context(params, task_id) {
        return format!(
            "✗ invalid task status.\nvalid task statuses: pending|in_progress|in_review|blocked|done|cancelled\nfix: codeterm plan task update {plan_id} {} --status pending\nuse --status-label for free-text nuance.",
            task_id.unwrap_or("<task_id>")
        );
    }
    format!(
        "✗ invalid plan status.\nvalid plan statuses: active|archived\nfix: codeterm plan update {plan_id} --status active"
    )
}

fn is_task_status_context(params: &Value, task_id: Option<&str>) -> bool {
    task_id.is_some()
        || params.get("task").is_some()
        || params.get("upsert_task").is_some()
        || params.get("mine").is_some()
        || params.get("unassigned").is_some()
}

fn render_not_found(plan_id: &str, loop_id: &str, params: &Value) -> String {
    if let Some(spec_id) = params.get("spec_id").and_then(|v| v.as_str()) {
        if let Some(section_id) = params.get("section_id").and_then(|v| v.as_str()) {
            return format!(
                "✗ no section '{section_id}' was found in spec '{spec_id}'.\nfix: codeterm spec show {spec_id}"
            );
        }
        if let Some(section_id) = params.get("spec_section_id").and_then(|v| v.as_str()) {
            return format!(
                "✗ no section '{section_id}' was found in spec '{spec_id}'.\nfix: codeterm spec show {spec_id}"
            );
        }
        return with_cross_daemon_hint(format!(
            "✗ no spec '{spec_id}' was found.\nfix: codeterm spec list --active"
        ));
    }
    if let Some(section_id) = params.get("section_id").and_then(|v| v.as_str()) {
        return format!(
            "✗ no section '{section_id}' was found in plan '{plan_id}'.\nfix: codeterm plan get {plan_id} --sections"
        );
    }
    if let Some(section_id) = params.get("plan_section_id").and_then(|v| v.as_str()) {
        return format!(
            "✗ no section '{section_id}' was found in plan '{plan_id}'.\nfix: codeterm plan get {plan_id} --sections"
        );
    }
    if params.get("loop_id").and_then(|v| v.as_str()).is_some() {
        return format!("✗ no loop '{loop_id}' was found.\nfix: codeterm plan state <plan_id>");
    }
    if params.get("pane_id").is_some() || params.get("session_id").is_some() {
        return format!(
            "✗ no heartbeat plan was found for this pane/session.\nfix: pass the plan explicitly:\n  codeterm orchestrator heartbeat show --plan {plan_id}\n  codeterm orchestrator heartbeat set --plan {plan_id} --focus \"<now>\""
        );
    }
    with_cross_daemon_hint(format!(
        "✗ no plan '{plan_id}' was found.\nfix: codeterm plan list --active"
    ))
}

fn render_transition_denied(data: &Value, plan_id: &str, task_id: Option<&str>) -> Option<String> {
    let from = data.get("from")?.as_str()?;
    let to = data.get("to")?.as_str()?;
    let task = task_id.unwrap_or("<task_id>");
    if from == "in_review" && to == "done" {
        return Some(format!(
            "✗ workers cannot mark an in-review task done with `record --status done`.\n\
`in_review` means the worker has reported and the Orchestrator must accept it explicitly.\n\
fix:\n  codeterm plan task close {plan_id} {task} --result \"<review result>\"\n\
If review found more work, bounce it instead:\n  codeterm plan task update {plan_id} {task} --status in_progress --status-label \"needs changes\""
        ));
    }
    Some(format!(
        "✗ task status transition {from} -> {to} is not allowed.\n\
fix: inspect the task and use the lifecycle command for that transition:\n  codeterm plan task close {plan_id} {task} --result \"<result>\"\n  codeterm plan task update {plan_id} {task} --status <status>"
    ))
}

fn render_io(data: &Value, plan_id: &str, params: &Value) -> String {
    let message = data
        .get("message")
        .and_then(|v| v.as_str())
        .unwrap_or("operation failed");
    if is_handoff_context(params) || message.contains("handoff") {
        return format!(
            "✗ handoff failed: {message}\nfix: inspect current handoffs, then retry with explicit inputs:\n  codeterm plan handoff list {plan_id}\n  codeterm plan handoff show {plan_id} --latest --markdown"
        );
    }
    format!(
        "✗ planner I/O failed: {message}\nfix: inspect the plan file state, then retry:\n  codeterm plan get {plan_id} --json\n  codeterm plan files {plan_id}"
    )
}

fn is_handoff_context(params: &Value) -> bool {
    params.get("selector").is_some()
        || params.get("next_actions").is_some()
        || params.get("foundational_rules").is_some()
        || params.get("open_decisions").is_some()
        || params
            .get("mode")
            .and_then(|v| v.as_str())
            .is_some_and(|mode| mode == "new" || mode == "compact")
}

fn append_cross_daemon_hint(message: String, params: &Value, raw_error: &str) -> String {
    if params.get("plan_id").and_then(|v| v.as_str()).is_none() {
        return message;
    }
    let lower = raw_error.to_ascii_lowercase();
    if lower.contains("pane not found")
        || lower.contains("plan not found")
        || lower.contains("task not found")
    {
        return with_cross_daemon_hint(message);
    }
    message
}

fn with_cross_daemon_hint(message: String) -> String {
    format!(
        "{message}\n\
hint: if this plan lives on the dev daemon but this pane is in prod, rerun with the dev daemon selected:\n  env -u CODETERM_API_TOKEN CODETERM_API_PORT=7686 codeterm ..."
    )
}

fn render_undeclared_kind(data: &Value, plan_id: &str, task_id: Option<&str>) -> Option<String> {
    let declared = data
        .get("declared")
        .and_then(|v| v.as_array())
        .map(|items| items.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
        .unwrap_or_default();
    let listed = if declared.is_empty() {
        "(none)".to_string()
    } else {
        declared.join("|")
    };
    let csv = if declared.is_empty() {
        "<new>".to_string()
    } else {
        format!("{},<new>", declared.join(","))
    };
    Some(format!(
        "✗ undeclared milestone kind.\nvalid kinds: {listed}\nfix: codeterm plan kinds set {plan_id} --kinds {csv}\nor declare and use it once with: codeterm plan milestone add {plan_id} {} --kind <new> --add-kind --text='status update'",
        task_id.unwrap_or("<task_id>")
    ))
}

fn render_open_subtasks(data: &Value, plan_id: &str, task_id: Option<&str>) -> Option<String> {
    let subtasks = data
        .get("subtasks")
        .and_then(|v| v.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(render_open_subtask)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let listed = if subtasks.is_empty() {
        "- <unknown subtask>".to_string()
    } else {
        subtasks.join("\n")
    };
    Some(format!(
        "✗ task '{}' still has open subtasks:\n{listed}\nfix: close or cancel the child tasks first, then retry:\n  codeterm plan task close {plan_id} <child_task_id>\n  codeterm plan task close {plan_id} {}",
        task_id.unwrap_or("<task_id>"),
        task_id.unwrap_or("<task_id>")
    ))
}

fn render_open_subtask(item: &Value) -> Option<String> {
    let id = item.get("id").and_then(|v| v.as_str())?;
    let name = item
        .get("name")
        .and_then(|v| v.as_str())
        .filter(|value| !value.is_empty())
        .unwrap_or(id);
    let status = item
        .get("status")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");
    Some(format!("- {name} ({id}) [{status}]"))
}

fn render_write_pending(data: &Value, plan_id: &str, task_id: Option<&str>) -> Option<String> {
    let operation = data
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("plan write");
    let committed_id = data
        .get("committed_id")
        .and_then(|v| v.as_str())
        .or(task_id)
        .unwrap_or("<task_id>");
    Some(format!(
        "✗ {operation} is pending after a committed write.\n\
do not retry blindly: the write may already exist as task '{committed_id}'.\n\
safe next step: read it back with `codeterm task show {committed_id}` or `codeterm plan get {plan_id} --json`."
    ))
}

fn render_readback_mismatch(data: &Value, plan_id: &str, task_id: Option<&str>) -> Option<String> {
    let operation = data
        .get("operation")
        .and_then(|v| v.as_str())
        .unwrap_or("plan write");
    let target = task_id.unwrap_or("<task_id>");
    Some(format!(
        "✗ {operation} did not read back the requested state for task '{target}'.\n\
do not retry blindly: inspect current state first with `codeterm task show {target}` or `codeterm plan get {plan_id} --json`."
    ))
}
