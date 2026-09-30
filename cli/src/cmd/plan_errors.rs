use crate::client::SocketCallError;
use crate::cmd::print::rpc_explanation;
use serde_json::Value;

pub(crate) const CROSS_DAEMON_HINT: &str = "hint: if this plan lives on the dev daemon but this Tab is in prod, rerun with the dev daemon selected:\n  env -u CODETERM_API_TOKEN CODETERM_API_PORT=7686 codeterm ...";

pub(crate) fn render_plan_call_error(err: SocketCallError, params: &Value) -> String {
    match err {
        SocketCallError::Rpc(rpc) if crate::protocol::is_version_skew(rpc.code) => rpc.message,
        SocketCallError::Rpc(rpc) => {
            let rendered = rpc
                .data
                .as_ref()
                .and_then(|data| render_plan_error(data, params));
            match rendered {
                Some(message) => message,
                None => {
                    append_cross_daemon_hint(rpc_explanation(&rpc.message), params, &rpc.message)
                }
            }
        }
        SocketCallError::Transport(message) => message,
    }
}

fn gate_key_list(data: &Value, field: &str) -> String {
    data.get(field)
        .and_then(|v| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|g| g.get("key").and_then(|k| k.as_str()))
                .collect::<Vec<_>>()
                .join(", ")
        })
        .unwrap_or_default()
}

fn render_regression_gate_blocked(data: &Value, plan_id: &str, task_id: Option<&str>) -> String {
    let gates = gate_key_list(data, "blocking_gates");
    let retrospectives = gate_key_list(data, "pending_retrospectives");
    let task = task_id.unwrap_or("<task_id>");
    let mut out = String::from(
        "✗ dispatch blocked: a terminal group's gates must clear before a non-blocker task dispatches.\nchoose one:\n",
    );
    if !gates.is_empty() || retrospectives.is_empty() {
        let gates = if gates.is_empty() {
            "<gate>"
        } else {
            gates.as_str()
        };
        out.push_str(&format!(
            "  clear regression gate(s) {gates} with a warm integration QA run: codeterm plan qa next {plan_id} --group <gate> --note \"<evidence>\"\n\
  record an authorized owner override: codeterm plan qa override {plan_id} --reason \"<why>\"\n"
        ));
    }
    if !retrospectives.is_empty() {
        out.push_str(&format!(
            "  record the group retrospective for {retrospectives}: codeterm plan groups close {plan_id} <group> --note \"<what worked, what failed, what changes next>\"\n"
        ));
    }
    out.push_str(&format!(
        "  mark {task} as the exception (gate-clearing or release-blocking): codeterm plan task update {plan_id} {task} --tags regression-gate"
    ));
    out
}

fn render_checkout_occupied(data: &Value) -> Option<String> {
    let task_id = data.get("occupying_task_id")?.as_str()?;
    let attempt_id = data.get("occupying_attempt_id")?.as_str()?;
    let checkout_path = data.get("checkout_path")?.as_str()?;
    let update_command = data.get("update_command")?.as_str()?;
    let dispatch_command = data.get("dispatch_command")?.as_str()?;
    Some(format!(
        "✗ dispatch refused: checkout '{checkout_path}' is occupied by live attempt \
         '{attempt_id}' of task '{task_id}'. No worker was spawned.\n\
assign an existing isolated lane, then dispatch again:\n  {update_command}\n  {dispatch_command}"
    ))
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
        "review_hold_required" => Some(render_review_hold_required(plan_id, task_id)),
        "undeclared_kind" => render_undeclared_kind(data, plan_id, task_id),
        "bad_status" => Some(render_bad_status(plan_id, task_id, params)),
        "bad_mode" => Some(format!(
            "✗ invalid plan mode.\nvalid modes: planning|approved|execution|complete\nfix: codeterm plan mode {plan_id} --set planning\nor approve a ready plan with: codeterm plan approve {plan_id}"
        )),
        "invalid_split_parent" => match data.get("reason").and_then(Value::as_str) {
            Some("terminal_parent") => Some(
                "✗ a terminal task cannot be split.\nfix: split an open top-level task instead."
                    .into(),
            ),
            Some("child_parent") => Some(
                "✗ a child task cannot be split.\nfix: split its top-level parent instead.".into(),
            ),
            _ => Some("✗ task split requires an open top-level parent.".into()),
        },
        "bad_priority" => Some(format!(
            "✗ invalid task priority.\nvalid priorities: p0|p1|p2|p3\nfix: codeterm plan task update {plan_id} {} --priority p2",
            task_id.unwrap_or("<task_id>")
        )),
        "executor_role_required" => Some(format!(
            "✗ a task must name who executes it, and an update cannot empty that.\n\
fix: codeterm plan task update {plan_id} {} --executor-role <role>\n\
presence is the contract; no vocabulary is enforced, so name the specialization \
that scopes the surface they own, e.g. rust-backend-dev",
            task_id.unwrap_or("<task_id>")
        )),
        "invalid_executor_name" => Some(format!(
            "✗ {}\nan executor name is one word of letters, given once at spawn and kept.\n\
fix: codeterm plan task update {plan_id} {} --personality Hopper",
            data.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("invalid executor name"),
            task_id.unwrap_or("<task_id>")
        )),
        "work_facet_violation" => Some(format!(
            "✗ {}
fix: codeterm plan task update {plan_id} {} --tag work:<frontend|backend|design|qa|infra|docs>",
            data.get("message").and_then(|v| v.as_str()).unwrap_or("invalid work tag"),
            task_id.unwrap_or("<task_id>")
        )),
        "not_found" => Some(render_not_found(plan_id, loop_id, params)),
        "invalid_model" | "ambiguous_model" => {
            let render = crate::cmd::model_error::render_model_catalogue_error(data)
                .unwrap_or_else(|| {
                    format!(
                        "✗ {}",
                        data["message"]
                            .as_str()
                            .unwrap_or("invalid model selection")
                    )
                });
            Some(render)
        }
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
        "open_subtasks" => render_open_subtasks(
            data,
            plan_id,
            task_id,
            params.get("status").and_then(Value::as_str),
        ),
        "task_creation_requires_approval" => Some(format!(
            "✗ this plan is still in planning mode.\n\
finish planning before generating execution tasks:\n\
  codeterm docs ORCHESTRATION\n\
  codeterm plan state {plan_id}\n\
  codeterm plan doctor {plan_id} --kind quality\n\
  codeterm plan approve {plan_id}\n\
then retry the task add. If you intentionally need a draft task, pass `--force`."
        )),
        "task_dispatch_requires_approval" => Some(format!(
            "✗ this plan is not approved for dispatch.\nfix: inspect and approve it before dispatching:\n  codeterm plan state {plan_id}\n  codeterm plan doctor {plan_id} --kind quality\n  codeterm plan approve {plan_id}"
        )),
        "dispatch_in_flight" => {
            let attempt_id = data
                .get("attempt_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<attempt_id>");
            let tab_id = data
                .get("tab_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<tab_id>");
            let task = task_id.unwrap_or("<task_id>");
            Some(format!(
                "✗ dispatch refused: live attempt '{attempt_id}' of task '{task}' is already running in tab '{tab_id}'. No worker was spawned.
do not dispatch again; follow the running attempt:
  codeterm plan task show {plan_id} {task}
  codeterm tab get-text --tab {tab_id}"
            ))
        }
        "unauthorized_gate_claim" => {
            let claimed = data
                .get("claimed")
                .and_then(|v| v.as_str())
                .unwrap_or("<command>");
            let authorized = data
                .get("authorized_gates")
                .and_then(|v| v.as_array())
                .map(|gates| {
                    gates
                        .iter()
                        .filter_map(|gate| gate.as_str())
                        .map(|gate| format!("  {gate}"))
                        .collect::<Vec<_>>()
                        .join("
")
                })
                .unwrap_or_default();
            let listing = if authorized.is_empty() {
                "this dispatch declared no gates, so no gate claim is accepted.".to_string()
            } else {
                format!("this dispatch authorized:
{authorized}")
            };
            Some(format!(
                "✗ gate '{claimed}' was not authorized for this attempt.
{listing}
fix: report only a declared gate, or ask the Orchestrator to dispatch with --authorized-gate '{claimed}'."
            ))
        }
        "active_plan_requires_team" => Some(format!(
            "✗ active plan '{plan_id}' requires a live Team.\nfix: codeterm team adopt-plan <team-id> {plan_id}"
        )),
        "heartbeat_requires_execution" => Some(format!(
            "✗ heartbeat is derived automatically from execution state.\n\
Use spec/plan sections for plan rules or mem for durable preferences:\n\
  codeterm docs ORCHESTRATION\n\
  codeterm plan state {plan_id}\n\
  codeterm mem save-preference --scope global --title \"CodeTerm planner preference\" \"<preference>\""
        )),
        "budget_exhausted" => {
            let count = data.get("count").and_then(|v| v.as_u64()).unwrap_or(0);
            let budget = data.get("budget").and_then(|v| v.as_u64()).unwrap_or(0);
            Some(format!(
                "✗ loop budget {count}/{budget} reached for loop '{loop_id}'; extend with codeterm plan loop set {loop_id} --budget N, or close the plan."
            ))
        }
        "in_progress_requires_tab" => Some(format!(
            "✗ rejected — nothing changed. in_progress needs an executor tab set in the same command.\nfix (atomic, pick one):\n  codeterm plan task dispatch {plan_id} {task} (binds tab+status together)\n  codeterm plan task update {plan_id} {task} --status in_progress --tab <tab-id>\nnote: CODETERM_STATE_INVARIANTS=off is an emergency escape hatch and was NOT applied here.",
            task = task_id.unwrap_or("<task_id>")
        )),
        "blocked_requires_label" => Some(format!(
            "✗ blocked is a claim, not an operator hold.\nfix: say what unblocks it, or open a decision gate:\n  codeterm plan task update {plan_id} {} --status blocked --status-label \"<what unblocks it>\"\n  codeterm plan decision ask {plan_id} \"<question>\"",
            task_id.unwrap_or("<task_id>")
        )),
        "task_paused_system_only" => Some(
            "✗ paused task status is reserved for decision-gate and provider-exhaustion system holds."
                .into(),
        ),
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
            "✗ waiting requires an open decision gate.\nfix: record the decision explicitly:\n  codeterm plan decision ask {plan_id} \"<question>\""
        )),
        "dependency_task_not_found" => Some(format!(
            "✗ dependency task '{}' was not found in plan '{plan_id}'.\nfix: list tasks, then choose an existing dependency:\n  codeterm plan task list {plan_id}\n  codeterm plan task update {plan_id} {} --depends-on <task_id>",
            data.get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<dependency-task-id>"),
            task_id.unwrap_or("<task_id>")
        )),
        "dependency_task_referenced" => Some(format!(
            "✗ task '{}' is still referenced by dependent tasks.\nfix: inspect the dependency graph before removing it:\n  codeterm task show {}",
            data.get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<task_id>"),
            data.get("task_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<task_id>")
        )),
        "dependency_self_reference" => Some(format!(
            "✗ a task cannot depend on itself.\nfix: remove the self-dependency or choose another task:\n  codeterm plan task update {plan_id} {} --clear-dependencies true",
            task_id.unwrap_or("<task_id>")
        )),
        "dependency_cycle" => Some(format!(
            "✗ task dependency would create a cycle.\nfix: inspect the task dependency graph, then remove or reorder the edge:\n  codeterm task show {}\n  codeterm plan task update {plan_id} {} --depends-on <acyclic-task-id>",
            task_id.unwrap_or("<task_id>"),
            task_id.unwrap_or("<task_id>")
        )),
        "team_already_has_active_plan" => Some(format!(
            "✗ team '{}' already owns active plan '{}'.\nfix: use another Team, or archive the existing plan first.",
            data.get("team_id").and_then(|v| v.as_str()).unwrap_or("<team-id>"),
            data.get("existing_plan_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<plan-id>")
        )),
        "plan_already_bound_to_team" => Some(format!(
            "✗ plan '{plan_id}' is already bound to team '{}'.\nfix: use that Team; a plan binds to exactly one Team.",
            data.get("team_id").and_then(|v| v.as_str()).unwrap_or("<team-id>")
        )),
        "plan_activation_requires_unarchive" => Some(format!(
            "✗ plan '{plan_id}' is archived.\nfix: codeterm plan unarchive {plan_id}"
        )),
        "plan_skill_already_exists" => Some(format!(
            "✗ plan skill '{}' already exists.\nfix: edit or show the existing skill instead of creating it again.",
            data.get("slug").and_then(|v| v.as_str()).unwrap_or("<slug>")
        )),
        "invalid_plan_skill_slug" => Some(
            "✗ plan skill slug is empty after normalization.\nfix: choose a slug containing letters or digits."
                .into(),
        ),
        "invalid_plan_text" => Some(format!(
            "✗ invalid plan text: {}\nfix: shorten the text or remove control characters, then retry.",
            data.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("text exceeds the plan bounds")
        )),
        "invalid_external_ref" => Some(format!(
            "✗ invalid external reference: {}",
            data.get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("inspect the supplied reference")
        )),
        "regression_gate_blocked" => Some(render_regression_gate_blocked(data, plan_id, task_id)),
        "checkout_occupied" => render_checkout_occupied(data),
        "override_reason_required" => Some(
            "✗ overriding a regression gate requires an explicit reason.\nfix: codeterm plan qa override <plan_id> --reason \"<why the frontier may advance past the gate>\"".into()
        ),
        "override_not_authorized" => Some(format!(
            "✗ only the plan Orchestrator (owner) may override a regression gate.\nfix: have the owner run the override, or clear the gate with a real QA run:\n  codeterm plan qa next {plan_id} --group <gate> --note \"<evidence>\""
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
        "goal_vector_required" => Some(format!(
            "\u{2717} a plan goal must name its vector.\nfix: pass --vector; every field you omit is kept unchanged:\n  codeterm plan goal set {plan_id} --vector \"<the one sentence this plan steers by>\""
        )),
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
        "review_continuation_attempt_not_found" => {
            let task = data
                .get("task_id")
                .and_then(|v| v.as_str())
                .or(task_id)
                .unwrap_or("<task_id>");
            Some(format!(
                "✗ task '{task}' was found, but its review attempt is missing or ambiguous.\nfix: inspect its attempts, then use the reopen path when one attempt resolves:\n  codeterm plan task show {plan_id} {task}\n  codeterm plan review bounce {plan_id} {task} --note \"<required changes>\""
            ))
        }
        "review_continuation_delivery_refused" => {
            let task = data
                .get("task_id")
                .and_then(|v| v.as_str())
                .or(task_id)
                .unwrap_or("<task_id>");
            let attempt = data
                .get("attempt_id")
                .and_then(|v| v.as_str())
                .unwrap_or("<attempt_id>");
            // The epochs are the whole story: the caller holds one the attempt
            // has already left, so say which, not just that it was refused.
            let drift = match (
                data.get("expected_epoch").and_then(|v| v.as_u64()),
                data.get("actual_epoch").and_then(|v| v.as_u64()),
            ) {
                (Some(expected), Some(actual)) => {
                    format!(" it is at epoch {actual}, the bounce was written for {expected}")
                }
                (Some(expected), None) => {
                    format!(" it carries no continuation epoch, the bounce was written for {expected}")
                }
                _ => String::new(),
            };
            Some(format!(
                "✗ the bounce was not delivered: attempt '{attempt}' on task '{task}' is no longer awaiting continuation.{drift}\nfix: re-read the task and bounce the attempt it actually holds:\n  codeterm plan task show {plan_id} {task}"
            ))
        }
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
    // A longer prefix only helps a truncated real id; for a nonexistent ref it
    // is dead-end advice, so point at the relevant list command instead.
    let list_hint = match kind {
        "plan" => "codeterm plan list --active",
        "spec" => "codeterm spec list --active",
        "task" => "codeterm plan task list <plan_id>",
        "section" => "codeterm plan get <plan_id> --sections",
        "spec section" => "codeterm spec show <spec_id>",
        "loop" => "codeterm plan state <plan_id>",
        _ => "list the matching entities, then retry with an exact id or handle",
    };
    Some(format!(
        "✗ {kind} '{reference}' not found.\nfix: {list_hint}"
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
            "✗ invalid task status.\nvalid task statuses: pending|in_progress|in_review|blocked|paused|done|cancelled\nfix: codeterm plan task update {plan_id} {} --status pending\nuse --status-label for free-text nuance.",
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
    if params.get("tab_id").is_some() || params.get("session_id").is_some() {
        return format!(
            "✗ no heartbeat plan was found for this tab/session.\nfix: pass the plan explicitly:\n  codeterm orchestrator heartbeat show --plan {plan_id}"
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
    let remedy = data
        .get("remedy")
        .and_then(|value| value.get("verb"))
        .and_then(Value::as_str);
    if from == "in_review" {
        return match remedy {
            Some("hold") => Some(render_review_hold_required(plan_id, task_id)),
            Some("accept") => Some(format!(
                "✗ the task is still in review.\nfix: codeterm plan review accept {plan_id} {task} --note \"<review result>\""
            )),
            Some("bounce") => Some(format!(
                "✗ the task is still in review.\nfix: codeterm plan review bounce {plan_id} {task} --note \"<required changes>\""
            )),
            Some("close") => Some(format!(
                "✗ the task is still in review.\nfix: codeterm plan task close {plan_id} {task} --result \"<review result>\""
            )),
            _ => None,
        };
    }
    Some(format!(
        "✗ task status transition {from} -> {to} is not allowed.\n\
fix: inspect the task and use the lifecycle command for that transition:\n  codeterm plan task close {plan_id} {task} --result \"<result>\"\n  codeterm plan task update {plan_id} {task} --status <status>"
    ))
}

fn render_review_hold_required(plan_id: &str, task_id: Option<&str>) -> String {
    let task = task_id.unwrap_or("<task_id>");
    format!(
        "✗ pause releases the assignment and lane, so it cannot pause an in-review task.\n\
fix: record the review hold instead:\n  codeterm plan review hold {plan_id} {task} --note \"<reason>\""
    )
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
    if lower.contains("tab not found")
        || lower.contains("plan not found")
        || lower.contains("task not found")
    {
        return with_cross_daemon_hint(message);
    }
    message
}

fn with_cross_daemon_hint(message: String) -> String {
    format!("{message}\n{CROSS_DAEMON_HINT}")
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

fn render_open_subtasks(
    data: &Value,
    plan_id: &str,
    task_id: Option<&str>,
    requested_status: Option<&str>,
) -> Option<String> {
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
    let parent_retry = if requested_status == Some("cancelled") {
        format!(
            "codeterm plan task update {plan_id} {} --status cancelled",
            task_id.unwrap_or("<task_id>")
        )
    } else {
        format!(
            "codeterm plan task close {plan_id} {}",
            task_id.unwrap_or("<task_id>")
        )
    };
    Some(format!(
        "✗ task '{}' still has open subtasks:\n{listed}\nfix: close or cancel the child tasks first, then retry:\n  codeterm plan task close {plan_id} <child_task_id>\n  codeterm plan task update {plan_id} <child_task_id> --status cancelled\n  {parent_retry}",
        task_id.unwrap_or("<task_id>"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn authoritative_model_refusal_renders_close_ids_and_refresh_command() {
        let message = render_plan_error(
            &json!({
                "type": "invalid_model",
                "provider": "codex",
                "requested_model": "gpt-9-bogus",
                "valid_model_ids": ["gpt-6-sol", "gpt-6-astra"],
                "message": "model id 'gpt-9-bogus' is not in provider 'codex' authoritative catalogue; closest catalogue ids: gpt-6-sol, gpt-6-astra",
            }),
            &json!({}),
        )
        .expect("invalid_model must have a renderer");
        assert!(message.contains("gpt-9-bogus"), "{message}");
        assert!(message.contains("gpt-6-sol"), "{message}");
        assert!(message.contains("codeterm agent models codex"), "{message}");
    }

    #[test]
    fn split_parent_error_uses_the_server_reason() {
        let terminal = render_plan_error(
            &json!({"type":"invalid_split_parent", "reason":"terminal_parent"}),
            &json!({"plan_id":"plan_a", "task_id":"task_a"}),
        )
        .expect("split parent errors must have a renderer");
        let child = render_plan_error(
            &json!({"type":"invalid_split_parent", "reason":"child_parent"}),
            &json!({"plan_id":"plan_a", "task_id":"task_a"}),
        )
        .expect("split parent errors must have a renderer");
        assert!(terminal.contains("terminal"), "{terminal}");
        assert!(child.contains("top-level"), "{child}");
    }

    #[test]
    fn open_subtasks_cancel_refusal_retries_the_requested_status() {
        let message = render_plan_error(
            &json!({"type":"open_subtasks", "subtasks":[{"id":"child_a", "name":"Child A"}]}),
            &json!({"plan_id":"plan_a", "task_id":"parent_a", "status":"cancelled"}),
        )
        .expect("open subtask errors must have a renderer");
        assert!(
            message.contains("codeterm plan task update plan_a parent_a --status cancelled"),
            "{message}"
        );
    }

    /// Selected by `data.type`, and the rendered remedy carries the flag the
    /// server named. Neither half asserts the refusal's prose.
    #[test]
    fn goal_vector_required_renders_the_flag_the_server_named() {
        let message = render_plan_error(
            &json!({ "type": "goal_vector_required", "remedy": { "flag": "--vector" } }),
            &json!({ "plan_id": "plan_a" }),
        )
        .expect("goal_vector_required must have a remedy renderer");
        assert!(message.contains("--vector"), "{message}");
        assert!(
            message.contains("codeterm plan goal set plan_a"),
            "{message}"
        );
    }

    /// The two review gates render distinct remedies, selected by their own
    /// discriminants; a shared renderer would hide the wrong-gate defect again.
    #[test]
    fn the_two_review_gates_render_distinct_remedies() {
        let context = json!({ "plan_id": "plan_a", "task_id": "task_b" });
        let round = render_plan_error(
            &json!({ "type": "review_round_exceeded", "round": 2, "max_rounds": 2 }),
            &context,
        )
        .unwrap();
        let scope = render_plan_error(
            &json!({ "type": "review_oracle_scope_expansion" }),
            &context,
        )
        .unwrap();
        assert_ne!(
            round, scope,
            "the round bound and the oracle scope must not render the same refusal"
        );
        for message in [&round, &scope] {
            assert!(
                message.contains("codeterm plan review override plan_a task_b"),
                "{message}"
            );
        }
    }

    #[test]
    fn checkout_occupied_error_is_actionable() {
        let update_command = "codeterm plan task update plan_a task_b --worktree (Read-Host 'Existing isolated worktree path') --branch (Read-Host 'Existing isolated branch') --base (Read-Host 'Base ref')";
        let data = json!({
            "type": "checkout_occupied",
            "occupying_task_id": "task_a",
            "occupying_attempt_id": "attempt_a",
            "checkout_path": "C:\\repo",
            "update_command": update_command,
            "dispatch_command": "codeterm plan task dispatch plan_a task_b",
        });
        let message =
            render_plan_error(&data, &json!({"plan_id": "plan_a", "task_id": "task_b"})).unwrap();

        assert!(message.contains("task_a"), "{message}");
        assert!(message.contains("attempt_a"), "{message}");
        assert!(message.contains("C:\\repo"), "{message}");
        assert!(
            message.contains("codeterm plan task update plan_a task_b"),
            "{message}"
        );
        assert!(message.contains("--worktree"), "{message}");
        assert!(message.contains("--branch"), "{message}");
        assert!(message.contains("--base"), "{message}");
        assert!(message.contains(update_command), "{message}");
        assert!(!message.contains("<isolated-"), "{message}");
        assert!(
            message.contains("codeterm plan task dispatch plan_a task_b"),
            "{message}"
        );
        assert!(message.contains("No worker was spawned"), "{message}");
    }

    #[test]
    fn active_team_conflict_renderer_preserves_server_identity() {
        let message = render_plan_error(
            &json!({
                "type": "team_already_has_active_plan",
                "team_id": "team_alpha",
                "existing_plan_id": "plan_existing",
            }),
            &json!({}),
        )
        .expect("team conflict renderer");

        assert!(message.contains("team_alpha"), "{message}");
        assert!(message.contains("plan_existing"), "{message}");
        assert!(!message.contains("<team-id>"), "{message}");
        assert!(!message.contains("<plan-id>"), "{message}");
    }

    #[test]
    fn spec_delete_on_nonexistent_id_says_not_found_not_longer_prefix() {
        let data = json!({
            "type": "ref_not_found",
            "kind": "spec",
            "reference": "sp_nope",
        });
        let message = render_plan_error(&data, &json!({})).expect("ref_not_found renders");
        assert!(message.contains("not found"), "{message}");
        assert!(
            !message.contains("longer unique prefix") && !message.contains("longer prefix"),
            "not-found should not suggest a longer prefix: {message}"
        );
        assert!(message.contains("codeterm spec list --active"), "{message}");
    }

    fn rpc_failure(code: i32, message: &str, data: Option<Value>) -> SocketCallError {
        SocketCallError::Rpc(crate::client::RpcError {
            code,
            message: message.into(),
            data,
        })
    }

    #[test]
    fn version_skew_plan_call_renders_the_daemon_message_verbatim() {
        let message = "client protocol v1, server requires >=4 — run ./refresh";
        let rendered = render_plan_call_error(
            rpc_failure(
                crate::protocol::VERSION_SKEW_CODE,
                message,
                Some(json!({
                    "type": "version_skew",
                    "client_protocol": 1,
                    "server_protocol": 4,
                    "recovery": "./refresh",
                })),
            ),
            &json!({"plan_id": "pl_1"}),
        );

        assert_eq!(rendered, message);
    }

    #[test]
    fn unstructured_rpc_failure_keeps_the_daemon_explanation_without_the_code() {
        let message =
            render_plan_call_error(rpc_failure(-32602, "plan 'zz' not found", None), &json!({}));

        assert!(message.contains("plan 'zz' not found"), "{message}");
        assert!(!message.contains("-32"), "{message}");
    }

    #[test]
    fn structured_plan_error_still_wins_over_the_raw_fallback() {
        let message = render_plan_call_error(
            rpc_failure(
                -32602,
                "resolve failed",
                Some(json!({ "type": "ref_not_found", "kind": "plan", "reference": "zz" })),
            ),
            &json!({}),
        );

        assert!(
            message.contains("fix: codeterm plan list --active"),
            "{message}"
        );
        assert!(!message.contains("resolve failed"), "{message}");
    }

    #[test]
    fn unexplained_rpc_failure_is_not_dressed_up_as_a_cause() {
        let message = render_plan_call_error(rpc_failure(-32000, "", None), &json!({}));

        assert!(!message.contains("-32"), "{message}");
        assert_eq!(message, crate::cmd::print::rpc_explanation(""));
    }

    #[test]
    fn ambiguous_spec_ref_still_suggests_a_longer_prefix() {
        let data = json!({
            "type": "ref_ambiguous",
            "kind": "spec",
            "reference": "sp",
            "candidates": [
                { "id": "sp_alpha", "label": "Alpha" },
                { "id": "sp_beta", "label": "Beta" },
            ],
        });
        let message = render_plan_error(&data, &json!({})).expect("ref_ambiguous renders");
        assert!(message.contains("longer prefix"), "{message}");
    }
}
