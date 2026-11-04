use super::header_edits::{append_field, set_field};
use super::*;
use crate::plan::gates::{DecisionGate, GateAnswer, GateStatus};
use crate::plan::{
    AgentRef, PlannerDeliveryPhase, PlannerInboxEntry, PlannerRequest, PlannerRequestState,
    TaskStatus,
};

pub(in crate::service_plan) struct GateAsk {
    pub question: String,
    pub context: String,
    pub blocking_task_ids: Vec<String>,
    pub routed_to_tab_id: Option<String>,
    pub asked_by_tab_id: Option<String>,
    pub caller_session_id: Option<String>,
    pub planner_request: bool,
}

pub(in crate::service_plan) enum GateChange {
    Answer {
        text: String,
        by_tab_id: Option<String>,
    },
    Escalate {
        note: Option<String>,
    },
}

impl PlanDatabase {
    pub(in crate::service_plan) fn ask_gate_rows(
        &self,
        plan_id: &str,
        ask: GateAsk,
        now_ms: u64,
    ) -> Result<(DecisionGate, Option<PlannerRequest>, CommittedRows), PlanError> {
        let mut output = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let gate = DecisionGate { id: mint_entity(tx, plan_id, "gate")?, question: ask.question, context: ask.context.trim().into(), blocking_task_ids: ask.blocking_task_ids,
                routed_to_tab_id: ask.routed_to_tab_id, asked_by_tab_id: ask.asked_by_tab_id,
                asked_at: now_ms, status: GateStatus::Open, answer: None, escalated_at: None, escalation_note: None };
            let pause_reason = format!("gate:{}: {}", gate.id, gate.question.chars().take(80).collect::<String>());
            for selector in &gate.blocking_task_ids {
                let Some(mut task) = optional_task(tx, plan_id, selector, receipt)? else { continue; };
                if task.status == TaskStatus::InReview {
                    return Err(PlanError::TransitionDenied { from: TaskStatus::InReview, to: TaskStatus::Paused });
                }
                if task.status.is_terminal() { continue; }
                task.status = TaskStatus::Paused;
                task.pause_reason = Some(pause_reason.clone());
                task.status_changed_at = now_ms;
                task.updated_at = now_ms;
                write_task(tx, plan_id, &task, receipt)?;
                append_milestone(tx, plan_id, &task.id, &Milestone {
                    id: mint_entity(tx, plan_id, "ms")?, from: AgentRef::from_tab(gate.asked_by_tab_id.as_deref().unwrap_or("orchestrator")),
                    kind: "decision".into(), text: format!("paused: {pause_reason}"), ts: now_ms,
                    links: vec![], loop_id: None, iteration: None, canvas: Default::default(), attachments: vec![],
                }, receipt)?;
                task_fields::queue_task_index(tx, plan_id, &task, false, true, receipt)?;
            }
            append_field(tx, plan_id, "$.gates", &gate, receipt)?;
            let request = if ask.planner_request {
                let request = PlannerRequest {
                    request_id: format!("gate:{}", gate.id),
                    payload: format!("[decision gate {}]\n{}\n{}\n\nAnswer with:\ncodeterm plan decision answer {} {} --answer \"<your answer>\"", gate.id, gate.question, ask.context, plan_id, gate.id),
                    payload_blob: None, caller_tab_id: gate.asked_by_tab_id.clone(), caller_session_id: ask.caller_session_id,
                    state: PlannerRequestState::Queued, created_at: now_ms, accepted_at: None, lease_expires_at: None,
                    delivery_attempts: 0, completed_at: None, failed_at: None, delivered_to_tab_id: None,
                    delivered_to_session_id: None, delivered_with_binding_nonce: None,
                };
                append_field(tx, plan_id, "$.planner_requests", &request, receipt)?;
                append_field(tx, plan_id, "$.planner_inbox", &PlannerInboxEntry { request_id: request.request_id.clone(),
                    phase: PlannerDeliveryPhase::PreSend, tab_id: None, session_id: None, binding_nonce: None,
                    transport: None, last_error: None, updated_at: now_ms }, receipt)?;
                Some(request)
            } else { None };
            output = Some((gate, request));
            Ok(())
        })?;
        let (gate, request) = output.ok_or(PlanError::GateNotFound)?;
        Ok((gate, request, receipt))
    }

    pub(in crate::service_plan) fn change_gate(
        &self,
        plan_id: &str,
        gate_id: &str,
        change: GateChange,
        now_ms: u64,
    ) -> Result<(DecisionGate, CommittedRows), PlanError> {
        let mut output = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let (position, document): (i64, String) = tx.query_row("SELECT entry.key, entry.value FROM plans p, json_each(p.header_json,'$.gates') entry WHERE p.id=?1 AND json_extract(entry.value,'$.id')=?2 ORDER BY entry.key LIMIT 1", params![plan_id, gate_id], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(db_error)?.ok_or(PlanError::GateNotFound)?;
            receipt.decoded_bytes += document.len();
            let mut gate: DecisionGate = serde_json::from_str(&document).map_err(json_error)?;
            if !gate.is_open() { return Err(PlanError::GateAlreadyAnswered); }
            match change {
                GateChange::Answer { text, by_tab_id } => {
                    gate.answer = Some(GateAnswer { text: text.trim().into(), by_tab_id: by_tab_id.clone(), at: now_ms });
                    gate.status = GateStatus::Answered;
                    for selector in &gate.blocking_task_ids {
                        let Some(mut task) = optional_task(tx, plan_id, selector, receipt)? else { continue; };
                        if task.status != TaskStatus::Paused { continue; }
                        task.status = TaskStatus::Pending;
                        task.status_changed_at = now_ms;
                        task.pause_reason = None;
                        task.updated_at = now_ms;
                        write_task(tx, plan_id, &task, receipt)?;
                        append_milestone(tx, plan_id, &task.id, &Milestone {
                            id: mint_entity(tx, plan_id, "ms")?, from: AgentRef::from_tab(by_tab_id.as_deref().unwrap_or("planner")),
                            kind: "decision".into(), text: format!("resumed: gate {} answered", gate.id), ts: now_ms,
                            links: vec![gate.id.clone()], loop_id: None, iteration: None, canvas: Default::default(), attachments: vec![],
                        }, receipt)?;
                        task_fields::queue_task_index(tx, plan_id, &task, false, true, receipt)?;
                    }
                }
                GateChange::Escalate { note } => {
                    gate.escalated_at = Some(now_ms);
                    gate.status = GateStatus::Escalated;
                    if let Some(note) = note.map(|note| note.trim().to_string()).filter(|note| !note.is_empty()) { gate.escalation_note = Some(note); }
                }
            }
            set_field(tx, plan_id, &format!("$.gates[{position}]"), &gate, receipt)?;
            output = Some(gate);
            Ok(())
        })?;
        Ok((output.ok_or(PlanError::GateNotFound)?, receipt))
    }
}

fn optional_task(
    tx: &Transaction<'_>,
    plan_id: &str,
    selector: &str,
    receipt: &mut CommittedRows,
) -> Result<Option<Task>, PlanError> {
    match read_task(tx, plan_id, selector, receipt) {
        Ok(task) => Ok(Some(task)),
        Err(PlanError::TaskNotFound) => Ok(None),
        Err(error) => Err(error),
    }
}

fn mint_entity(tx: &Transaction<'_>, plan_id: &str, prefix: &str) -> Result<String, PlanError> {
    crate::plan::ids::try_mint_id(prefix, |candidate| {
        tx.query_row("SELECT EXISTS(SELECT 1 FROM plan_tasks WHERE plan_id=?1 AND task_id=?2) OR EXISTS(SELECT 1 FROM plan_task_milestones WHERE plan_id=?1 AND json_extract(row_json,'$.id')=?2) OR EXISTS(SELECT 1 FROM plan_strides WHERE plan_id=?1 AND json_extract(row_json,'$.id')=?2) OR EXISTS(SELECT 1 FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1 AND j.key IN ('id','handle') AND j.value=?2) OR EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.gates') g WHERE p.id=?1 AND json_extract(g.value,'$.id')=?2)", params![plan_id, candidate], |row| row.get(0)).map_err(db_error)
    })
}

#[cfg(test)]
mod tests;
