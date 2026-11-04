use super::header_edits::{append_field, set_field};
use super::*;
use crate::plan::{Spec, SpecLink};
mod create;
pub(super) use create::insert_plan;
pub(in crate::service_plan) use create::SpecPlanCreate;

pub(in crate::service_plan) enum SpecLinkChange {
    Link(SpecLink),
    Unlink {
        spec_id: String,
        spec_section_id: Option<String>,
        plan_section_id: Option<String>,
    },
}

impl PlanDatabase {
    #[cfg(test)]
    pub(in crate::service_plan) fn remove_spec_test_sidecars(
        &self,
        spec: &Spec,
    ) -> Result<(), PlanError> {
        use crate::service_plan::{paths, spec_store};
        let sidecar_root = paths::spec_sidecar_root(spec);
        if paths::test_project_root(sidecar_root).is_none() {
            return Ok(());
        }
        let root = paths::project_root(sidecar_root);
        let directory = paths::specs_dir(&root).map_err(super::super::io_error)?;
        let _ = std::fs::remove_file(spec_store::spec_yaml_path(&directory, &spec.id));
        if let Some(plan_id) = spec.plan_id.as_deref() {
            let location: Option<(String,String)> = self.connection.lock().query_row("SELECT CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END,status FROM plans WHERE id=?1", [plan_id], |row| Ok((row.get(0)?,row.get(1)?))).optional().map_err(db_error)?;
            if let Some((root, status)) = location {
                let status = serde_json::from_value(serde_json::Value::String(status))
                    .map_err(json_error)?;
                let directory = paths::plan_state_dir(
                    &paths::project_root(std::path::Path::new(&root)),
                    &status,
                    plan_id,
                )
                .map_err(super::super::io_error)?;
                let _ = std::fs::remove_file(spec_store::spec_yaml_path_in_plan(&directory));
            }
        }
        Ok(())
    }

    pub(in crate::service_plan) fn delete_spec_rows(
        &self,
        id: &str,
        now_ms: u64,
    ) -> Result<(Spec, Vec<CommittedRows>), PlanError> {
        let result = self.commit_entity_rows(id, "delete_spec", now_ms, |tx, operation_id| {
            let mut work = CommittedRows::default();
            let spec = read(tx, id, &mut work)?;
            let mut statement = tx.prepare_cached("SELECT p.id FROM plans p WHERE json_extract(p.header_json,'$.primary_spec_id')=?1 OR EXISTS(SELECT 1 FROM json_each(p.header_json,'$.spec_refs') r WHERE r.value=?1) OR EXISTS(SELECT 1 FROM json_each(p.header_json,'$.spec_links') l WHERE json_extract(l.value,'$.spec_id')=?1) ORDER BY p.id").map_err(db_error)?;
            let ids: Vec<String> = statement.query_map([id], |row| row.get(0)).map_err(db_error)?.collect::<Result<_,_>>().map_err(db_error)?;
            work.projected_text_bytes += ids.iter().map(String::len).sum::<usize>();
            let mut receipts = Vec::new();
            for plan_id in ids {
                let mut receipt = row_receipt(tx, &plan_id, now_ms, operation_id)?;
                unlink(tx, &plan_id, id, None, None, true, &mut receipt)?;
                touch_header(tx, &mut receipt)?;
                receipts.push(receipt);
            }
            tx.execute("DELETE FROM specs WHERE id=?1", [id]).map_err(db_error)?;
            work.spec_rows = 1;
            receipts.push(work);
            Ok((spec,receipts))
        })?;
        for receipt in &result.1 {
            self.record_row_change(receipt);
        }
        Ok(result)
    }

    pub(in crate::service_plan) fn edit_spec_rows<T>(
        &self,
        id: &str,
        now_ms: u64,
        change: impl FnOnce(&mut Spec) -> Result<T, PlanError>,
    ) -> Result<(T, CommittedRows), PlanError> {
        self.commit_entity_rows(id, "edit_spec", now_ms, |tx, _| {
            let mut receipt = CommittedRows::default();
            let mut spec = read(tx, id, &mut receipt)?;
            let result = change(&mut spec)?;
            spec.updated_at = now_ms;
            write(tx, &mut spec, &mut receipt)?;
            Ok((result, receipt))
        })
    }

    pub(in crate::service_plan) fn change_spec_link(
        &self,
        plan_id: &str,
        change: SpecLinkChange,
        now_ms: u64,
    ) -> Result<CommittedRows, PlanError> {
        self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            apply_link(tx, plan_id, change, now_ms, receipt)
        })
    }
}

pub(super) fn apply_link(
    tx: &Transaction<'_>,
    plan_id: &str,
    change: SpecLinkChange,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    match change {
        SpecLinkChange::Link(link) => {
            let mut spec = read(tx, &link.spec_id, receipt)?;
            if !crate::service_plan::mutations::has_section(&spec.sections, &link.spec_section_id) {
                return Err(PlanError::NotFound);
            }
            let exists: bool = tx
                .query_row(
                    &format!(
                        "SELECT EXISTS(SELECT 1 FROM ({}) WHERE id=?2 OR handle=?2)",
                        super::super::read_projections::SECTION_ROWS
                    ),
                    params![plan_id, link.plan_section_id],
                    |row| row.get(0),
                )
                .map_err(db_error)?;
            if !exists {
                return Err(PlanError::NotFound);
            }
            if let Some(owner) = spec.plan_id.as_deref() {
                if owner != plan_id {
                    return Err(PlanError::Io(format!(
                        "spec {} already owned by plan {owner}",
                        spec.id
                    )));
                }
            } else {
                spec.plan_id = Some(plan_id.into());
                spec.updated_at = now_ms;
                write(tx, &mut spec, receipt)?;
            }
            let reference: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.spec_refs') r WHERE p.id=?1 AND r.value=?2)", params![plan_id,spec.id], |row| row.get(0)).map_err(db_error)?;
            if !reference {
                append_field(tx, plan_id, "$.spec_refs", &spec.id, receipt)?;
            }
            let primary: bool = tx.query_row("SELECT json_extract(header_json,'$.primary_spec_id') IS NOT NULL FROM plans WHERE id=?1", [plan_id], |row| row.get(0)).map_err(db_error)?;
            if !primary {
                set_field(tx, plan_id, "$.primary_spec_id", &spec.id, receipt)?;
            }
            let linked: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.spec_links') l WHERE p.id=?1 AND json_extract(l.value,'$.spec_id')=?2 AND json_extract(l.value,'$.spec_section_id')=?3 AND json_extract(l.value,'$.plan_section_id')=?4)", params![plan_id,link.spec_id,link.spec_section_id,link.plan_section_id], |row| row.get(0)).map_err(db_error)?;
            if !linked {
                append_field(tx, plan_id, "$.spec_links", &link, receipt)?;
            }
        }
        SpecLinkChange::Unlink {
            spec_id,
            spec_section_id,
            plan_section_id,
        } => unlink(
            tx,
            plan_id,
            &spec_id,
            spec_section_id.as_deref(),
            plan_section_id.as_deref(),
            false,
            receipt,
        )?,
    }
    Ok(())
}

pub(super) fn unlink(
    tx: &Transaction<'_>,
    plan_id: &str,
    spec_id: &str,
    spec_section: Option<&str>,
    plan_section: Option<&str>,
    deleting: bool,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    receipt.header_rows += tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.spec_links',(SELECT json_group_array(json(value)) FROM json_each(header_json,'$.spec_links') WHERE NOT (json_extract(value,'$.spec_id')=?2 AND (?3 IS NULL OR json_extract(value,'$.spec_section_id')=?3) AND (?4 IS NULL OR json_extract(value,'$.plan_section_id')=?4)))) WHERE id=?1", params![plan_id,spec_id,spec_section,plan_section]).map_err(db_error)?;
    let remains: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM plans p,json_each(p.header_json,'$.spec_links') l WHERE p.id=?1 AND json_extract(l.value,'$.spec_id')=?2)", params![plan_id,spec_id], |row| row.get(0)).map_err(db_error)?;
    if !remains {
        receipt.header_rows += tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.spec_refs',(SELECT json_group_array(value) FROM json_each(header_json,'$.spec_refs') WHERE value<>?2)) WHERE id=?1", params![plan_id,spec_id]).map_err(db_error)?;
        receipt.header_rows += tx.execute("UPDATE plans SET header_json=json_set(header_json,'$.primary_spec_id',CASE WHEN ?3 THEN NULL ELSE json_extract(header_json,'$.spec_refs[0]') END) WHERE id=?1 AND json_extract(header_json,'$.primary_spec_id')=?2", params![plan_id,spec_id,deleting]).map_err(db_error)?;
    }
    Ok(())
}

pub(super) fn read(
    tx: &Transaction<'_>,
    id: &str,
    receipt: &mut CommittedRows,
) -> Result<Spec, PlanError> {
    let document: String = tx
        .query_row("SELECT row_json FROM specs WHERE id=?1", [id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(db_error)?
        .ok_or(PlanError::NotFound)?;
    receipt.decoded_bytes += document.len();
    serde_json::from_str(&document).map_err(json_error)
}

pub(super) fn write(
    tx: &Transaction<'_>,
    spec: &mut Spec,
    receipt: &mut CommittedRows,
) -> Result<(), PlanError> {
    if spec.canonical_project_path.trim().is_empty() {
        spec.canonical_project_path = spec.project_path.clone();
    }
    crate::service_plan::tag_model::normalize_sections_for_spec(&mut spec.sections);
    text_bounds::validate_spec(spec)?;
    let document = serde_json::to_string(spec).map_err(json_error)?;
    receipt.serialized_bytes += document.len();
    receipt.spec_rows += tx.execute("INSERT INTO specs(id,plan_id,project_path,updated_at,row_json) VALUES (?1,?2,?3,?4,?5) ON CONFLICT(id) DO UPDATE SET plan_id=excluded.plan_id,project_path=excluded.project_path,updated_at=excluded.updated_at,row_json=excluded.row_json", params![spec.id,spec.plan_id,spec.project_path,spec.updated_at,document]).map_err(db_error)?;
    if let Some(previous) = receipt
        .specs
        .iter_mut()
        .find(|previous| previous.id == spec.id)
    {
        *previous = spec.clone();
    } else {
        receipt.specs.push(spec.clone());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
