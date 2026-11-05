use super::read_projections::{column, decode, record_query};
use super::{db_error, PlanDatabase};
use crate::plan::coverage::{
    CoverageSectionInput, CoverageTaskInput, PlanCoverageInput, SpecCoverageInput,
};
use crate::plan::{PlanStatus, SpecLink};
use crate::service_plan::PlanError;
use rusqlite::{Connection, OptionalExtension};
use std::collections::HashMap;

impl PlanDatabase {
    pub(in crate::service_plan) fn coverage_input(
        &self,
        id: &str,
    ) -> Result<Option<PlanCoverageInput>, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        record_query();
        let status: Option<String> = connection
            .query_row(
                "SELECT json_quote(status) FROM plans WHERE id=?1",
                [id],
                |row| column(row, 0),
            )
            .optional()
            .map_err(db_error)?;
        let Some(status) = status else {
            connection.commit().map_err(db_error)?;
            return Ok(None);
        };
        if decode::<PlanStatus>(id, &status)? == PlanStatus::Archived {
            connection.commit().map_err(db_error)?;
            return Ok(None);
        }
        let sections = section_metadata(&connection, id)?;
        record_query();
        let tasks = {
            let mut statement = connection.prepare_cached(
                "SELECT json_object('name',CASE WHEN coalesce(json_array_length(row_json,'$.section_ids'),0)=0 THEN json_extract(row_json,'$.name') ELSE '' END,
                 'status',status,'section_ids',json(CASE WHEN json_type(row_json,'$.section_ids') IS NULL THEN '[]' ELSE row_json->'$.section_ids' END))
                 FROM plan_tasks WHERE plan_id=?1 ORDER BY position"
            ).map_err(db_error)?;
            let rows = statement
                .query_map([id], |row| column::<String>(row, 0))
                .map_err(db_error)?;
            rows.map(|row| decode(id, &row.map_err(db_error)?))
                .collect::<Result<Vec<CoverageTaskInput>, PlanError>>()?
        };
        connection.commit().map_err(db_error)?;
        Ok(Some(PlanCoverageInput {
            sections,
            tasks,
            spec_links: Vec::new(),
        }))
    }

    pub(in crate::service_plan) fn spec_links(
        &self,
        id: &str,
    ) -> Result<Vec<(String, SpecLink)>, PlanError> {
        spec_links(&self.connection.lock(), id)
    }

    pub(in crate::service_plan) fn spec_coverage_input(
        &self,
        id: &str,
    ) -> Result<Option<(SpecCoverageInput, Vec<PlanCoverageInput>)>, PlanError> {
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        let result = spec_coverage_input(&connection, id)?;
        connection.commit().map_err(db_error)?;
        Ok(result)
    }
}

pub(super) fn spec_coverage_input(
    connection: &Connection,
    id: &str,
) -> Result<Option<(SpecCoverageInput, Vec<PlanCoverageInput>)>, PlanError> {
    record_query();
    let document: Option<String> = connection.query_row(
            "SELECT json_object('id',id,'sections',json((SELECT json_group_array(json_object('id',json_extract(value,'$.id'),'title',json_extract(value,'$.title'))) FROM (
                SELECT j.value FROM json_tree(s.row_json,'$.sections') j WHERE j.type='object'
                AND (j.path='$.sections' OR j.path LIKE '%.subsections') ORDER BY j.id)))) FROM specs s WHERE id=?1",
            [id],|row| column(row,0),
        ).optional().map_err(db_error)?;
    let Some(document) = document else {
        return Ok(None);
    };
    let spec = decode(id, &document)?;
    let mut plans = Vec::<PlanCoverageInput>::new();
    let mut indices = HashMap::<String, usize>::new();
    for (plan_id, link) in spec_links(connection, id)? {
        let index = *indices.entry(plan_id).or_insert_with(|| {
            let index = plans.len();
            plans.push(PlanCoverageInput::default());
            index
        });
        plans[index].spec_links.push(link);
    }
    record_query();
    {
        let mut statement = connection.prepare_cached(
                "WITH links AS MATERIALIZED (
                    SELECT p.id AS plan_id,json_extract(l.value,'$.plan_section_id') AS section_id
                    FROM plans p,json_each(p.header_json,'$.spec_links') l
                    WHERE json_extract(l.value,'$.spec_id')=?1
                 ), task_facts AS MATERIALIZED (
                    SELECT plan_id,task_id,position,status,
                        CASE WHEN json_type(row_json,'$.section_ids') IS NULL THEN '[]'
                        ELSE row_json->'$.section_ids' END AS section_ids
                    FROM plan_tasks WHERE plan_id IN (SELECT plan_id FROM links)
                 ), task_sections AS MATERIALIZED (
                    SELECT t.plan_id,t.task_id,sid.value AS section_id
                    FROM task_facts t,json_each(t.section_ids) sid
                 ) SELECT t.plan_id,json_object('name','','status',t.status,'section_ids',json(t.section_ids))
                 FROM task_facts t WHERE EXISTS(SELECT 1 FROM task_sections s JOIN links l
                    ON l.plan_id=s.plan_id AND l.section_id=s.section_id
                    WHERE s.plan_id=t.plan_id AND s.task_id=t.task_id)
                 ORDER BY t.plan_id,t.position"
            ).map_err(db_error)?;
        let rows = statement
            .query_map([id], |row| {
                Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
            })
            .map_err(db_error)?;
        for row in rows {
            let (plan_id, document) = row.map_err(db_error)?;
            let index = indices
                .get(&plan_id)
                .ok_or_else(|| PlanError::Io("spec coverage task has no matching link".into()))?;
            plans[*index].tasks.push(decode(&plan_id, &document)?);
        }
    }
    Ok(Some((spec, plans)))
}

fn section_metadata(
    connection: &Connection,
    id: &str,
) -> Result<Vec<CoverageSectionInput>, PlanError> {
    record_query();
    let mut statement = connection.prepare_cached(
        "SELECT json_object('id',json_extract(j.value,'$.id'),'title',json_extract(j.value,'$.title'))
         FROM plan_sections s,json_tree(s.row_json) j WHERE s.plan_id=?1 AND j.type='object'
         AND (j.fullkey='$' OR j.path LIKE '%.subsections') ORDER BY s.position,j.id"
    ).map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| column::<String>(row, 0))
        .map_err(db_error)?;
    rows.map(|row| decode(id, &row.map_err(db_error)?))
        .collect()
}

pub(super) fn spec_links(
    connection: &Connection,
    id: &str,
) -> Result<Vec<(String, SpecLink)>, PlanError> {
    record_query();
    let mut statement = connection
        .prepare_cached(
            "SELECT p.id,l.value FROM plans p,json_each(p.header_json,'$.spec_links') l
         WHERE json_extract(l.value,'$.spec_id')=?1 ORDER BY p.updated_at DESC,p.id,l.key",
        )
        .map_err(db_error)?;
    let rows = statement
        .query_map([id], |row| {
            Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
        })
        .map_err(db_error)?;
    rows.map(|row| {
        let (plan, document) = row.map_err(db_error)?;
        let link = decode(&plan, &document)?;
        Ok((plan, link))
    })
    .collect()
}
