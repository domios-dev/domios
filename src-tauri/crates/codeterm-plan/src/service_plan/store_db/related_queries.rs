use super::read_projections::{column, decode, record_query, SECTION_ROWS};
use super::{db_error, PlanDatabase};
use crate::plan::EffortLink;
use crate::service_plan::search_related::{RelatedGroup, RelatedUnit};
use crate::service_plan::{PlanError, PlanMatch, TaskMatch};
use rusqlite::{params, Connection, OptionalExtension};

impl PlanDatabase {
    pub(in crate::service_plan) fn related_projection(
        &self,
        id: &str,
    ) -> Result<Vec<RelatedGroup>, PlanError> {
        let holder = PlanMatch::Any(vec![
            PlanMatch::Ids(vec![id.into()]),
            PlanMatch::SectionId(id.into()),
            PlanMatch::Task(TaskMatch {
                fields: vec![("$.id", id.into())],
                ..Default::default()
            }),
        ]);
        let (clause, arguments) = super::super::plan_match::compile(&holder);
        let mut guard = self.connection.lock();
        let connection = guard.transaction().map_err(db_error)?;
        record_query();
        let selected: Option<(String,String)>=connection.query_row(
            &format!("SELECT p.id,coalesce(json_extract(p.header_json,'$.lineage'),'[]') FROM plans p WHERE {clause} ORDER BY p.updated_at DESC,p.id LIMIT 1"),
            rusqlite::params_from_iter(arguments),|row|Ok((column(row,0)?,column(row,1)?)),
        ).optional().map_err(db_error)?;
        let Some((plan_id, lineage)) = selected else {
            connection.commit().map_err(db_error)?;
            return Ok(vec![]);
        };
        let lineage: Vec<EffortLink> = decode(&plan_id, &lineage)?;
        let mut groups = Vec::new();
        record_query();
        let sections: Option<String>=connection.query_row(
            "SELECT coalesce(json_extract(row_json,'$.section_ids'),'[]') FROM plan_tasks WHERE plan_id=?1 AND task_id=?2",
            params![plan_id,id],|row|column(row,0),
        ).optional().map_err(db_error)?;
        if let Some(sections) = sections {
            let sections: Vec<String> = decode(&plan_id, &sections)?;
            let units = sections
                .into_iter()
                .map(|id| {
                    let title = plan_section_title(&connection, &plan_id, &id)?;
                    Ok(unit(id, "plan_section", title))
                })
                .collect::<Result<Vec<_>, PlanError>>()?;
            groups.push(RelatedGroup {
                edge_type: "task_sections".into(),
                units,
            });
            groups.push(RelatedGroup {
                edge_type: "subtasks".into(),
                units: task_units(&connection, &plan_id, id, false)?,
            });
        } else if plan_section_title(&connection, &plan_id, id)?.is_some() {
            groups.push(RelatedGroup {
                edge_type: "section_tasks".into(),
                units: task_units(&connection, &plan_id, id, true)?,
            });
            let links = {
                record_query();
                let mut statement=connection.prepare_cached(
                    "SELECT json_extract(l.value,'$.spec_id'),json_extract(l.value,'$.spec_section_id')
                     FROM plans p,json_each(p.header_json,'$.spec_links') l WHERE p.id=?1 AND json_extract(l.value,'$.plan_section_id')=?2 ORDER BY l.key"
                ).map_err(db_error)?;
                let rows = statement
                    .query_map(params![plan_id, id], |row| {
                        Ok((column::<String>(row, 0)?, column::<String>(row, 1)?))
                    })
                    .map_err(db_error)?;
                rows.collect::<Result<Vec<_>, _>>().map_err(db_error)?
            };
            let mut units = Vec::new();
            for (spec_id, section_id) in links {
                record_query();
                let title: Option<String>=connection.query_row(
                    "SELECT json_extract(j.value,'$.title') FROM specs s,json_tree(s.row_json,'$.sections') j
                     WHERE s.id=?1 AND j.type='object' AND (j.path='$.sections' OR j.path LIKE '%.subsections')
                       AND json_extract(j.value,'$.id')=?2 ORDER BY j.id LIMIT 1",
                    params![spec_id,section_id],|row|column(row,0),
                ).optional().map_err(db_error)?;
                units.push(unit(section_id, "spec_section", title));
            }
            groups.push(RelatedGroup {
                edge_type: "spec_links".into(),
                units,
            });
        }
        let mut units = Vec::new();
        for link in lineage {
            record_query();
            let title: Option<String> = connection
                .query_row("SELECT title FROM plans WHERE id=?1", [&link.to], |row| {
                    column(row, 0)
                })
                .optional()
                .map_err(db_error)?;
            units.push(unit(link.to, "plan", title));
        }
        groups.push(RelatedGroup {
            edge_type: "lineage".into(),
            units,
        });
        connection.commit().map_err(db_error)?;
        Ok(groups)
    }
}

fn unit(id: String, kind: &str, title: Option<String>) -> RelatedUnit {
    let removed = title.is_none();
    RelatedUnit {
        id,
        kind: kind.into(),
        title: title.unwrap_or_else(|| "removed".into()),
        removed,
    }
}

fn plan_section_title(
    connection: &Connection,
    plan_id: &str,
    id: &str,
) -> Result<Option<String>, PlanError> {
    record_query();
    connection.query_row(&format!("SELECT title FROM ({SECTION_ROWS}) WHERE id=?2 ORDER BY position,traversal LIMIT 1"),params![plan_id,id],|row|column(row,0)).optional().map_err(db_error)
}

fn task_units(
    connection: &Connection,
    plan_id: &str,
    id: &str,
    section: bool,
) -> Result<Vec<RelatedUnit>, PlanError> {
    record_query();
    let predicate = if section {
        "EXISTS(SELECT 1 FROM json_each(t.row_json,'$.section_ids') WHERE value=?2)"
    } else {
        "json_extract(t.row_json,'$.parent_task_id')=?2"
    };
    let mut statement=connection.prepare_cached(&format!("SELECT task_id,json_extract(row_json,'$.name') FROM plan_tasks t WHERE plan_id=?1 AND {predicate} ORDER BY position")).map_err(db_error)?;
    let rows = statement
        .query_map(params![plan_id, id], |row| {
            Ok(unit(column(row, 0)?, "task", Some(column(row, 1)?)))
        })
        .map_err(db_error)?;
    rows.collect::<Result<Vec<_>, _>>().map_err(db_error)
}
