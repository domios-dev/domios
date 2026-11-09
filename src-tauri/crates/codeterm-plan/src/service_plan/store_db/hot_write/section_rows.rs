use super::*;
use crate::plan::Section;
use crate::service_plan::{mutations, search_index::SectionIndexDelta, tag_model};
use std::collections::HashSet;

pub(in crate::service_plan) enum SectionChange {
    Add {
        section: Section,
        parent: Option<String>,
    },
    Update {
        selector: String,
        title: Option<String>,
        body: Option<String>,
        tags: Vec<String>,
        primary: Option<String>,
        handle: Option<String>,
    },
    Remove {
        selector: String,
    },
    ReplaceRoot {
        section: Section,
        existing: bool,
    },
    Canvas {
        selector: String,
        x: Option<f64>,
        y: Option<f64>,
    },
}

impl PlanDatabase {
    pub(in crate::service_plan) fn change_section(
        &self,
        plan_id: &str,
        change: SectionChange,
        now_ms: u64,
    ) -> Result<(Vec<String>, CommittedRows), PlanError> {
        let mut removed = Vec::new();
        let receipt = self.hot_transaction(plan_id, now_ms, |tx, receipt| {
            removed = apply(tx, plan_id, change, now_ms, receipt)?;
            Ok(())
        })?;
        Ok((removed, receipt))
    }
    pub(in crate::service_plan) fn edit_section_rows<T>(
        &self,
        plan_id: &str,
        selector: &str,
        now_ms: u64,
        change: impl FnOnce(&mut Section) -> Result<T, PlanError>,
    ) -> Result<(T, CommittedRows), PlanError> {
        let mut output = None;
        let receipt=self.hot_transaction(plan_id,now_ms,|tx,receipt| {
            let document:String=tx.query_row("SELECT row_json FROM plan_sections s WHERE plan_id=?1 AND EXISTS(SELECT 1 FROM json_tree(s.row_json) j WHERE j.type='object' AND (j.fullkey='$' OR j.path LIKE '%.subsections') AND (json_extract(j.value,'$.id')=?2 OR json_extract(j.value,'$.handle')=?2)) ORDER BY position LIMIT 1",params![plan_id,selector],|row|row.get(0)).optional().map_err(db_error)?.ok_or(PlanError::ElementNotFound)?;
            receipt.decoded_bytes += document.len();
            let mut root:Section=serde_json::from_str(&document).map_err(json_error)?;
            output=Some(change(selected_mut(std::slice::from_mut(&mut root),selector).ok_or(PlanError::ElementNotFound)?)?);
            apply(tx,plan_id,SectionChange::ReplaceRoot { section:root,existing:true },now_ms,receipt)?;
            Ok(())
        })?;
        Ok((output.ok_or(PlanError::ElementNotFound)?, receipt))
    }
}

pub(super) fn apply(
    tx: &Transaction<'_>,
    plan_id: &str,
    change: SectionChange,
    now_ms: u64,
    receipt: &mut CommittedRows,
) -> Result<Vec<String>, PlanError> {
    let mut removed = Vec::new();
    let selector = match &change {
        SectionChange::Add { parent, .. } => parent.as_deref(),
        SectionChange::ReplaceRoot { .. } => None,
        SectionChange::Update { selector, .. }
        | SectionChange::Remove { selector }
        | SectionChange::Canvas { selector, .. } => Some(selector.as_str()),
    };
    let (mut roots, mut existing) = if let Some(selector) = selector {
        let (id, document): (String, String) = tx.query_row("SELECT section_id,row_json FROM plan_sections s WHERE plan_id=?1 AND EXISTS(SELECT 1 FROM json_tree(s.row_json) j WHERE j.type='object' AND (j.fullkey='$' OR j.path LIKE '%.subsections') AND (json_extract(j.value,'$.id')=?2 OR json_extract(j.value,'$.handle')=?2)) ORDER BY position LIMIT 1", params![plan_id, selector], |row| Ok((row.get(0)?, row.get(1)?))).optional().map_err(db_error)?.ok_or(PlanError::NotFound)?;
        receipt.decoded_bytes += document.len();
        (
            vec![serde_json::from_str::<Section>(&document).map_err(json_error)?],
            Some(id),
        )
    } else {
        (Vec::new(), None)
    };
    match change {
        SectionChange::ReplaceRoot {
            section,
            existing: persisted,
        } => {
            if persisted {
                existing = Some(section.id.clone());
            }
            roots.push(section);
        }
        SectionChange::Add { section, parent } => {
            if let Some(parent) = parent {
                if mutations::push_subsection(&mut roots, &parent, section).is_some() {
                    return Err(PlanError::NotFound);
                }
            } else {
                roots.push(section);
            }
        }
        SectionChange::Update {
            selector,
            title,
            body,
            tags,
            primary,
            handle,
        } => {
            let id = selected(&roots, &selector)
                .ok_or(PlanError::NotFound)?
                .id
                .clone();
            if !mutations::update_section_fields(&mut roots, &id, title.as_deref(), body.as_deref())
            {
                return Err(PlanError::NotFound);
            }
            mutations::update_section_tags(
                &mut roots,
                &id,
                &tags,
                primary.as_deref(),
                handle.as_deref(),
            );
        }
        SectionChange::Canvas { selector, x, y } => {
            let section = selected_mut(&mut roots, &selector).ok_or(PlanError::NotFound)?;
            section.canvas.x = x;
            section.canvas.y = y;
        }
        SectionChange::Remove { selector } => {
            collect_ids(
                selected(&roots, &selector).ok_or(PlanError::NotFound)?,
                &mut removed,
            );
            if !mutations::rm_section_tree(&mut roots, &selector) {
                return Err(PlanError::NotFound);
            }
        }
    }
    let mut used = external_handles(tx, plan_id, existing.as_deref(), receipt)?;
    tag_model::normalize_sections(&mut roots, &mut used);
    let section = roots.pop();
    if let Some(section) = &section {
        text_bounds::validate_stored_section(section)?;
        let document = encode(section, receipt)?;
        receipt.hashed_bytes += document.len();
        receipt.section_rows += if existing.is_some() {
                    tx.execute("UPDATE plan_sections SET content_hash=?3,row_json=?4 WHERE plan_id=?1 AND section_id=?2", params![plan_id, section.id, store_rows::row_hash(&document), document])
                } else {
                    tx.execute("INSERT INTO plan_sections(plan_id,section_id,position,content_hash,row_json) VALUES (?1,?2,(SELECT coalesce(max(position)+1,0) FROM plan_sections WHERE plan_id=?1),?3,?4)", params![plan_id, section.id, store_rows::row_hash(&document), document])
                }.map_err(db_error)?;
    } else if let Some(id) = &existing {
        receipt.section_rows += tx
            .execute(
                "DELETE FROM plan_sections WHERE plan_id=?1 AND section_id=?2",
                params![plan_id, id],
            )
            .map_err(db_error)?;
    }
    let (title, status, root): (String, String, String) = tx.query_row("SELECT title,status,CASE WHEN trim(coalesce(json_extract(header_json,'$.canonical_project_path'),''))='' THEN project_path ELSE json_extract(header_json,'$.canonical_project_path') END FROM plans WHERE id=?1", [plan_id], |row| Ok((row.get(0)?,row.get(1)?,row.get(2)?))).map_err(db_error)?;
    receipt.projected_text_bytes += title.len() + status.len() + root.len();
    let total: usize = tx
        .query_row(
            &format!(
                "SELECT count(*) FROM ({})",
                super::super::read_projections::SECTION_ROWS
            ),
            [plan_id],
            |row| row.get(0),
        )
        .map_err(db_error)?;
    let ordinal = if let Some(section) = &section {
        tx.query_row(&format!("WITH sections AS ({}) SELECT 1+(SELECT count(*) FROM sections before WHERE before.position<selected.position) FROM sections selected WHERE id=?2 LIMIT 1", super::super::read_projections::SECTION_ROWS), params![plan_id,section.id], |row| row.get(0)).map_err(db_error)?
    } else {
        0
    };
    receipt.section_search.push(SectionIndexDelta {
        plan_id: plan_id.into(),
        title,
        status,
        project_root: root.into(),
        updated_at: now_ms,
        section,
        removed: removed.clone(),
        ordinal,
        total,
    });
    Ok(removed)
}

fn selected<'a>(sections: &'a [Section], selector: &str) -> Option<&'a Section> {
    for section in sections {
        if section.id == selector || section.handle.as_deref() == Some(selector) {
            return Some(section);
        }
        if let Some(found) = selected(&section.subsections, selector) {
            return Some(found);
        }
    }
    None
}

pub(super) fn selected_mut<'a>(
    sections: &'a mut [Section],
    selector: &str,
) -> Option<&'a mut Section> {
    for section in sections {
        if section.id == selector || section.handle.as_deref() == Some(selector) {
            return Some(section);
        }
        if let Some(found) = selected_mut(&mut section.subsections, selector) {
            return Some(found);
        }
    }
    None
}

fn collect_ids(section: &Section, ids: &mut Vec<String>) {
    ids.push(section.id.clone());
    for child in &section.subsections {
        collect_ids(child, ids);
    }
}

fn external_handles(
    tx: &Transaction<'_>,
    plan_id: &str,
    excluded_root: Option<&str>,
    receipt: &mut CommittedRows,
) -> Result<HashSet<String>, PlanError> {
    let mut statement = tx.prepare_cached("SELECT j.value FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1 AND (?2 IS NULL OR s.section_id<>?2) AND j.key='handle' AND j.type='text' UNION ALL SELECT json_extract(row_json,'$.handle') FROM plan_tasks WHERE plan_id=?1 AND json_type(row_json,'$.handle')='text'").map_err(db_error)?;
    let mut handles = HashSet::new();
    for row in statement
        .query_map(params![plan_id, excluded_root], |row| {
            row.get::<_, String>(0)
        })
        .map_err(db_error)?
    {
        let handle = row.map_err(db_error)?;
        receipt.projected_text_bytes += handle.len();
        handles.insert(handle);
    }
    Ok(handles)
}

#[cfg(test)]
mod tests;
