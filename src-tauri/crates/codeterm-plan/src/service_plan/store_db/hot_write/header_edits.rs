use super::*;
use crate::plan::{
    EffortLink, OrchestratorRef, PlanMode, PlannerRef, PlanningAuthor, PlanningState,
    PlanningTransition, TagFacet, TaskGroup, TransferRecord,
};

pub(in crate::service_plan) enum HeaderChange {
    Planner(Option<PlannerRef>),
    Mode(PlanMode),
    Planning {
        to: PlanningState,
        by: PlanningAuthor,
        note: Option<String>,
    },
    Kinds(Vec<MilestoneKind>),
    Groups(Vec<TaskGroup>),
    Facets(Vec<TagFacet>),
    Link {
        to: String,
        remove: bool,
    },
    Effort(EffortLink),
    Owner {
        to: OrchestratorRef,
        reason: String,
    },
    RefreshOwner {
        tab_id: String,
        session_id: Option<String>,
        title: Option<String>,
    },
}

impl PlanDatabase {
    pub(in crate::service_plan) fn change_header(
        &self,
        plan_id: &str,
        change: HeaderChange,
        now_ms: u64,
    ) -> Result<(bool, CommittedRows), PlanError> {
        let mut changed = true;
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            match change {
                HeaderChange::Planner(value) => set_field(tx, plan_id, "$.planner", &value, receipt)?,
                HeaderChange::Kinds(value) => set_field(tx, plan_id, "$.milestone_kinds", &value, receipt)?,
                HeaderChange::Groups(value) => set_field(tx, plan_id, "$.task_groups", &value, receipt)?,
                HeaderChange::Facets(value) => set_field(tx, plan_id, "$.tag_facets", &value, receipt)?,
                HeaderChange::Mode(mode) => {
                    let previous: PlanMode = read_field(tx, plan_id, "$.mode", receipt)?;
                    if mode == PlanMode::Paused && previous != PlanMode::Paused {
                        set_field(tx, plan_id, "$.paused_from_mode", &previous, receipt)?;
                    } else if mode != PlanMode::Paused {
                        set_field(tx, plan_id, "$.paused_from_mode", &Option::<PlanMode>::None, receipt)?;
                    }
                    set_field(tx, plan_id, "$.mode", &mode, receipt)?;
                }
                HeaderChange::Planning { to, by, note } => {
                    let from: PlanningState = read_field(tx, plan_id, "$.planning_state", receipt)?;
                    if from != to {
                        append_field(tx, plan_id, "$.planning_log", &PlanningTransition { from, to, by, note, at: now_ms }, receipt)?;
                        set_field(tx, plan_id, "$.planning_state", &to, receipt)?;
                        let mode: PlanMode = read_field(tx, plan_id, "$.mode", receipt)?;
                        match (to.allows_execution_tasks(), mode) {
                            (true, PlanMode::Planning) => set_field(tx, plan_id, "$.mode", &PlanMode::Approved, receipt)?,
                            (false, PlanMode::Approved) => set_field(tx, plan_id, "$.mode", &PlanMode::Planning, receipt)?,
                            _ => {}
                        }
                    }
                }
                HeaderChange::Link { to, remove } => {
                    if remove {
                        receipt.header_rows += tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.links',(SELECT json_group_array(value) FROM json_each(header_json,'$.links') WHERE value != ?2)) WHERE id=?1", params![plan_id, to]).map_err(db_error)?;
                    } else {
                        let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plans p, json_each(p.header_json,'$.links') WHERE p.id=?1 AND value=?2)", params![plan_id, to], |row| row.get(0)).map_err(db_error)?;
                        if !exists { append_field(tx, plan_id, "$.links", &to, receipt)?; }
                    }
                }
                HeaderChange::Effort(link) => {
                    let document = encode(&link, receipt)?;
                    let exists: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plans p, json_each(p.header_json,'$.lineage') WHERE p.id=?1 AND json_extract(value,'$.to')=?2 AND json_extract(value,'$.rel')=json_extract(?3,'$.rel'))", params![plan_id, link.to, document], |row| row.get(0)).map_err(db_error)?;
                    if !exists { append_field(tx, plan_id, "$.lineage", &link, receipt)?; }
                }
                HeaderChange::Owner { to, reason } => {
                    let from: OrchestratorRef = read_field(tx, plan_id, "$.owner", receipt)?;
                    append_field(tx, plan_id, "$.owner_history", &TransferRecord { from, to: to.clone(), ts: now_ms, reason }, receipt)?;
                    set_field(tx, plan_id, "$.owner", &to, receipt)?;
                }
                HeaderChange::RefreshOwner { tab_id, session_id, title } => {
                    let mut owner: OrchestratorRef = read_field(tx, plan_id, "$.owner", receipt)?;
                    changed = false;
                    if owner.tab_id.as_deref() == Some(tab_id.as_str()) {
                        let title = title.or_else(|| owner.title.clone());
                        if owner.session_id != session_id || owner.title != title {
                            owner.session_id = session_id;
                            owner.title = title;
                            set_field(tx, plan_id, "$.owner", &owner, receipt)?;
                            changed = true;
                        }
                    }
                }
            }
            Ok(())
        })?;
        Ok((changed, receipt))
    }
}

pub(super) fn read_field<T: serde::de::DeserializeOwned + Default>(
    tx: &Transaction<'_>,
    plan_id: &str,
    path: &str,
    receipt: &mut CommittedRows,
) -> Result<T, PlanError> {
    let document: String = tx.query_row("SELECT coalesce(json_quote(json_extract(header_json,?2)),'null') FROM plans WHERE id=?1", params![plan_id, path], |row| row.get(0)).map_err(db_error)?;
    receipt.decoded_bytes += document.len();
    serde_json::from_str::<Option<T>>(&document)
        .map(|value| value.unwrap_or_default())
        .map_err(json_error)
}

pub(super) fn set_field(
    tx: &Transaction<'_>,
    plan_id: &str,
    path: &str,
    value: &impl serde::Serialize,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let document = encode(value, receipt)?;
    receipt.header_rows += tx
        .execute(
            "UPDATE plans SET header_json=json_set(header_json,?2,json(?3)) WHERE id=?1",
            params![plan_id, path, document],
        )
        .map_err(db_error)?;
    Ok(())
}

pub(super) fn append_field(
    tx: &Transaction<'_>,
    plan_id: &str,
    path: &str,
    value: &impl serde::Serialize,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    let document = encode(value, receipt)?;
    receipt.header_rows += tx.execute("UPDATE plans SET header_json=json_set(header_json,?2,json_insert(coalesce(json_extract(header_json,?2),'[]'),'$[#]',json(?3))) WHERE id=?1", params![plan_id, path, document]).map_err(db_error)?;
    Ok(())
}

#[cfg(test)]
mod tests;
