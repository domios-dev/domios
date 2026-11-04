use super::*;
use crate::plan::{AgentRef, RegressionGateOverride, TaskGroup};
use crate::service_plan::{
    milestone_gates::{self, RegressionGate},
    section_links::{self, SectionLinkInput, SectionRelinkReport},
};

fn section_input(
    tx: &Transaction<'_>,
    id: &str,
    tasks: bool,
    receipt: &mut CommittedRows,
) -> Result<SectionLinkInput, PlanError> {
    let input = super::super::section_link_queries::section_link_input(tx, id, tasks)?;
    receipt.projected_text_bytes += input
        .sections
        .iter()
        .map(|section| {
            section.id.len()
                + section.title.len()
                + section.handle.as_ref().map_or(0, String::len)
                + section.tags.iter().map(String::len).sum::<usize>()
        })
        .sum::<usize>();
    receipt.projected_text_bytes += input
        .tasks
        .iter()
        .map(|task| {
            task.id.len()
                + task.name.len()
                + task.handle.as_ref().map_or(0, String::len)
                + task.group.as_ref().map_or(0, String::len)
        })
        .sum::<usize>();
    Ok(input)
}

impl PlanDatabase {
    pub(in crate::service_plan) fn link_task_section_rows(
        &self,
        plan_id: &str,
        selector: &str,
        selectors: &[String],
        now_ms: u64,
    ) -> Result<(Vec<String>, CommittedRows), PlanError> {
        let mut linked = vec![];
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let input = section_input(tx, plan_id, false, receipt)?;
            linked = section_links::resolve_selectors(&input, selectors);
            let id = resolve_task(tx, plan_id, selector, receipt)?;
            let task = read_task(tx, plan_id, &id, receipt)?;
            let mut ids = task.section_ids;
            for id in &linked {
                if !ids.contains(id) {
                    ids.push(id.clone());
                }
            }
            task_fields::apply(
                tx,
                plan_id,
                &id,
                TaskFields {
                    section_ids: Some(ids),
                    ..Default::default()
                },
                now_ms,
                receipt,
            )
        })?;
        Ok((linked, receipt))
    }

    pub(in crate::service_plan) fn relink_task_section_rows(
        &self,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<(SectionRelinkReport, CommittedRows), PlanError> {
        let mut report = SectionRelinkReport {
            applied: vec![],
            suggestions: vec![],
            unlinked_count: 0,
        };
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let input = section_input(tx, plan_id, true, receipt)?;
            report = section_links::section_link_preview(&input);
            for suggestion in &report.suggestions {
                task_fields::apply(
                    tx,
                    plan_id,
                    &suggestion.task_id,
                    TaskFields {
                        section_ids: Some(vec![suggestion.section_id.clone()]),
                        ..Default::default()
                    },
                    now_ms,
                    receipt,
                )?;
            }
            report.applied = std::mem::take(&mut report.suggestions);
            Ok(())
        })?;
        Ok((report, receipt))
    }

    pub(in crate::service_plan) fn record_group_regression_rows(
        &self,
        plan_id: &str,
        key: &str,
        note: &str,
        from: AgentRef,
        now_ms: u64,
    ) -> Result<(RegressionGate, CommittedRows), PlanError> {
        let mut gate = None;
        let receipt=self.hot_transaction(plan_id,now_ms,|tx,receipt| {
            let document:String=tx.query_row("SELECT g.value FROM plans p,json_each(p.header_json,'$.task_groups') g WHERE p.id=?1 AND json_extract(g.value,'$.key')=?2 ORDER BY g.key LIMIT 1",params![plan_id,key],|row|row.get(0)).optional().map_err(db_error)?.ok_or(PlanError::GroupNotFound)?;
            receipt.decoded_bytes += document.len();
            let group:TaskGroup=serde_json::from_str(&document).map_err(json_error)?;
            gate=Some(RegressionGate { key:group.key,label:group.label });
            append_stride(tx,plan_id,&Milestone { id:crate::plan::ids::short_id("ms"),from,kind:crate::service_plan::prompts_plan::REGRESSION_MILESTONE_KIND.into(),text:note.into(),ts:now_ms,links:vec![milestone_gates::regression_link(key)],loop_id:None,iteration:None,canvas:Default::default(),attachments:vec![] },receipt)
        })?;
        Ok((gate.ok_or(PlanError::GroupNotFound)?, receipt))
    }

    pub(in crate::service_plan) fn record_regression_override_rows(
        &self,
        plan_id: &str,
        gates: Vec<String>,
        reason: &str,
        actor: AgentRef,
        now_ms: u64,
    ) -> Result<(RegressionGateOverride, CommittedRows), PlanError> {
        let mut record = None;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            let owner: AgentRef = header_edits::read_field(tx, plan_id, "$.owner", receipt)?;
            if owner
                .tab_id
                .as_deref()
                .is_some_and(|owner| actor.tab_id.as_deref() != Some(owner))
            {
                return Err(PlanError::OverrideNotAuthorized);
            }
            let gates = if gates.is_empty() {
                let input =
                    super::super::document_queries::regression::regression_gate_input(tx, plan_id)?
                        .ok_or(PlanError::NotFound)?;
                receipt.projected_text_bytes += input
                    .groups
                    .iter()
                    .map(|group| group.key.len() + group.label.len())
                    .sum::<usize>();
                milestone_gates::blocking_regression_gates(input)
                    .into_iter()
                    .map(|gate| gate.key)
                    .collect()
            } else {
                gates
            };
            let entry = RegressionGateOverride {
                actor,
                reason: reason.into(),
                gates,
                ts: now_ms,
            };
            header_edits::append_field(
                tx,
                plan_id,
                "$.regression_gate_overrides",
                &entry,
                receipt,
            )?;
            record = Some(entry);
            Ok(())
        })?;
        Ok((record.ok_or(PlanError::NotFound)?, receipt))
    }
}

#[cfg(test)]
mod tests;
