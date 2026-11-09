use super::*;
use crate::plan::Section;
use crate::service_plan::ingest::{self, PlanIngestResult};

impl PlanDatabase {
    pub(in crate::service_plan) fn ingest_section_rows(
        &self,
        plan_id: &str,
        markdown: &str,
        now_ms: u64,
    ) -> Result<(PlanIngestResult, CommittedRows), PlanError> {
        let mut result = PlanIngestResult { sections: vec![] };
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            result = apply(tx, plan_id, markdown, now_ms, receipt)?;
            Ok(())
        })?;
        Ok((result, receipt))
    }
}

pub(super) fn apply(
    tx: &Transaction<'_>,
    plan_id: &str,
    markdown: &str,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<PlanIngestResult, PlanError> {
    let parsed = ingest::parse_markdown_sections(markdown);
    let mut result = PlanIngestResult { sections: vec![] };
    for incoming in parsed {
        let document:Option<String> = tx.query_row("SELECT row_json FROM plan_sections WHERE plan_id=?1 AND (json_extract(row_json,'$.handle')=?2 OR trim(json_extract(row_json,'$.title'),?4)=?3) ORDER BY position LIMIT 1",params![plan_id,incoming.handle,incoming.title.trim(),super::super::status_queries::WHITESPACE],|row|row.get(0)).optional().map_err(db_error)?;
        let existing = document.is_some();
        let mut roots = if let Some(document) = document {
            receipt.decoded_bytes += document.len();
            vec![serde_json::from_str::<Section>(&document).map_err(json_error)?]
        } else {
            vec![]
        };
        ingest::upsert_sections(
            &mut roots,
            std::slice::from_ref(&incoming),
            &mut result.sections,
        );
        let section = roots.pop().ok_or(PlanError::NotFound)?;
        section_rows::apply(
            tx,
            plan_id,
            SectionChange::ReplaceRoot { section, existing },
            now_ms,
            receipt,
        )?;
    }
    Ok(result)
}
