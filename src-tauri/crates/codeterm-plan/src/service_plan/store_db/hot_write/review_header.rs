use super::*;
use crate::plan::{PlanEvidenceReporterGrant, ReviewCapacityGrant};

pub(in crate::service_plan) struct ReviewHeaderRows {
    pub id: String,
    pub task_id: Option<String>,
    pub active_reviews: usize,
    pub credit: i64,
    pub evidence_reporters: Vec<PlanEvidenceReporterGrant>,
    pub capacity_grants: Vec<ReviewCapacityGrant>,
    pub strides: Vec<Milestone>,
}

impl PlanDatabase {
    pub(in crate::service_plan) fn with_review_header<T>(
        &self,
        plan_id: &str,
        task: Option<&str>,
        evidence: Option<(&str, &str, &str)>,
        now: u64,
        change: impl FnOnce(&mut ReviewHeaderRows) -> Result<T, PlanError>,
    ) -> Result<(T, CommittedRows), PlanError> {
        let mut result = None;
        let receipt = self.hot_transaction(plan_id, now, |tx, receipt| {
            let task_id = task.map(|task| resolve_task(tx, plan_id, task, receipt)).transpose()?;
            let credit: i64 = tx.query_row("SELECT coalesce(json_extract(header_json,'$.review_policy.credit'),0) FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
            let active_reviews: usize = tx.query_row("SELECT count(*) FROM plan_tasks WHERE plan_id=?1 AND status='in_review'", [plan_id], |row| row.get(0)).map_err(db_error)?;
            let reporter: Option<String> = if let Some((tab, session, digest)) = evidence {
                tx.query_row("SELECT j.value FROM plans p,json_each(p.header_json,'$.evidence_reporters') j WHERE p.id=?1 AND json_extract(j.value,'$.tab_id')=?2 AND json_extract(j.value,'$.session_id')=?3 AND json_extract(j.value,'$.session_binding_digest')=?4 LIMIT 1", params![plan_id,tab,session,digest], |row| row.get(0)).optional().map_err(db_error)?
            } else { None };
            let capacity: Option<String> = if let Some(id) = &task_id {
                tx.query_row("SELECT j.value FROM plans p,json_each(p.header_json,'$.review_policy.capacity_grants') j WHERE p.id=?1 AND json_extract(j.value,'$.task_id')=?2 LIMIT 1", params![plan_id,id], |row| row.get(0)).optional().map_err(db_error)?
            } else { None };
            let mut rows = ReviewHeaderRows {
                id: plan_id.into(), task_id, active_reviews, credit,
                evidence_reporters: decode_optional(reporter, receipt)?,
                capacity_grants: decode_optional(capacity, receipt)?,
                strides: Vec::new(),
            };
            let reporter_count = rows.evidence_reporters.len();
            let capacity_count = rows.capacity_grants.len();
            result = Some(change(&mut rows)?);
            if rows.credit != credit {
                header_edits::set_field(tx, plan_id, "$.review_policy.credit", &rows.credit, receipt)?;
            }
            for grant in &rows.evidence_reporters[reporter_count..] {
                header_edits::append_field(tx, plan_id, "$.evidence_reporters", grant, receipt)?;
            }
            for grant in &rows.capacity_grants[capacity_count..] {
                header_edits::append_field(tx, plan_id, "$.review_policy.capacity_grants", grant, receipt)?;
            }
            for stride in &rows.strides { append_stride(tx, plan_id, stride, receipt)?; }
            Ok(())
        })?;
        Ok((result.ok_or(PlanError::NotFound)?, receipt))
    }
}

fn decode_optional<T: serde::de::DeserializeOwned>(
    json: Option<String>,
    receipt: &mut CommittedRows,
) -> Result<Vec<T>, PlanError> {
    json.into_iter()
        .map(|json| {
            receipt.decoded_bytes += json.len();
            serde_json::from_str(&json).map_err(json_error)
        })
        .collect()
}
