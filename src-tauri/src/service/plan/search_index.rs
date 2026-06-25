use std::path::{Path, PathBuf};

use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::mem::{embeddings, search, store};
use crate::plan::{Milestone, Plan, PlanStatus, Section, Spec, Task};

const INDEX_FILE: &str = "plan-index.sqlite";
const PLAN_INDEX_BUSY_RETRY_MS: &[u64] = &[25, 50, 100, 200, 400];

#[cfg(not(test))]
const PLAN_INDEX_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(test)]
const PLAN_INDEX_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(75);

#[derive(Debug, Clone, Serialize)]
pub struct PlanSearchHit {
    pub source: &'static str,
    pub score: f64,
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub scope: String,
    pub plan_id: Option<String>,
    pub spec_id: Option<String>,
    pub unit_id: String,
    pub unit_title: String,
    pub parent_title: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelatedGroup {
    pub edge_type: String,
    pub units: Vec<RelatedUnit>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RelatedUnit {
    pub id: String,
    pub kind: String,
    pub title: String,
    #[serde(default)]
    pub removed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct PlanReindexResult {
    pub project: String,
    pub scope: String,
    pub plans: usize,
    pub specs: usize,
    pub units: usize,
}

#[derive(Debug, Clone)]
struct Unit {
    id: String,
    kind: &'static str,
    title: String,
    body: String,
    scope: String,
    plan_id: Option<String>,
    spec_id: Option<String>,
    unit_id: String,
    unit_title: String,
    parent_title: String,
    status: Option<String>,
}

impl super::PlanStore {
    pub fn mirror_plan_index(&self, plan: &Plan) -> Result<(), String> {
        mirror_plan(plan, false)
    }

    pub fn mirror_spec_index(&self, spec: &Spec) -> Result<(), String> {
        mirror_spec(spec, false)
    }

    pub fn search_plan_index(
        &self,
        project: &Path,
        q: &search::SearchQuery,
    ) -> Result<Vec<PlanSearchHit>, String> {
        search_project(project, q)
    }

    pub fn reindex_project_index(&self, project: &Path) -> Result<PlanReindexResult, String> {
        let root = super::paths::project_root(project);
        let root_text = root.display().to_string();
        let plans = self
            .list_full()
            .into_iter()
            .filter(|plan| plan.project_path == root_text)
            .collect::<Vec<_>>();
        let specs = self
            .list_specs_full()
            .into_iter()
            .filter(|spec| spec.project_path == root_text)
            .collect::<Vec<_>>();
        let scope = project_scope(&root);
        delete_units_by_scope(&root, &scope)?;
        let mut units = 0usize;
        for plan in &plans {
            units += plan_units(plan, &scope).len();
            mirror_plan(plan, false)?;
        }
        for spec in &specs {
            units += spec_units(spec, &scope).len();
            mirror_spec(spec, false)?;
        }
        Ok(PlanReindexResult {
            project: root_text,
            scope,
            plans: plans.len(),
            specs: specs.len(),
            units,
        })
    }

    pub fn related_units(&self, id: &str) -> Vec<RelatedGroup> {
        related_units(&self.list_full(), &self.list_specs_full(), id)
    }

    pub fn list_specs_full(&self) -> Vec<Spec> {
        self.specs
            .read()
            .values()
            .map(|s| s.lock().clone())
            .collect()
    }
}

pub fn project_index_path(project_root: &Path) -> PathBuf {
    super::paths::project_root(project_root)
        .join(".codeterm")
        .join(INDEX_FILE)
}

pub fn project_scope(project_root: &Path) -> String {
    let root = super::paths::project_root(project_root);
    let mut hasher = Sha256::new();
    hasher.update(root.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    format!(
        "plan_{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
        digest[0], digest[1], digest[2], digest[3], digest[4], digest[5], digest[6], digest[7]
    )
}

pub fn mirror_plan(plan: &Plan, sync_embed: bool) -> Result<(), String> {
    let root = Path::new(&plan.project_path);
    let units = plan_units(plan, &project_scope(root));
    replace_units(root, &format!("planunit_{}_%", plan.id), units, sync_embed)
}

pub fn mirror_spec(spec: &Spec, sync_embed: bool) -> Result<(), String> {
    let root = Path::new(&spec.project_path);
    let units = spec_units(spec, &project_scope(root));
    replace_units(root, &format!("planunit_{}_%", spec.id), units, sync_embed)
}

pub fn delete_plan_units(plan: &Plan) -> Result<(), String> {
    delete_units(
        Path::new(&plan.project_path),
        &format!("planunit_{}_%", plan.id),
    )
}

fn open_index(project_root: &Path) -> Result<Connection, String> {
    let db_path = project_index_path(project_root);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create plan index dir: {e}"))?;
    }
    embeddings::ensure_extension_registered();
    let conn = Connection::open(&db_path).map_err(|e| format!("open plan index: {e}"))?;
    conn.busy_timeout(PLAN_INDEX_BUSY_TIMEOUT)
        .map_err(|e| format!("set plan index busy_timeout: {e}"))?;
    store::init_schema(&conn).map_err(|e| format!("init plan index schema: {e}"))?;
    let _ = embeddings::init_extension(&conn);
    Ok(conn)
}

fn replace_units(
    project_root: &Path,
    prefix_like: &str,
    units: Vec<Unit>,
    sync_embed: bool,
) -> Result<(), String> {
    retry_index_write(|| replace_units_once(project_root, prefix_like, &units, sync_embed))
}

fn replace_units_once(
    project_root: &Path,
    prefix_like: &str,
    units: &[Unit],
    sync_embed: bool,
) -> Result<(), String> {
    let mut conn = open_index(project_root)?;
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    delete_units_tx(&tx, prefix_like)?;
    for unit in units {
        insert_unit(&tx, unit)?;
        if sync_embed {
            let _ = embeddings::index(&tx, &unit.id, &unit.body);
        }
    }
    tx.commit().map_err(|e| e.to_string())
}

fn retry_index_write<T, F>(mut f: F) -> Result<T, String>
where
    F: FnMut() -> Result<T, String>,
{
    for delay_ms in PLAN_INDEX_BUSY_RETRY_MS {
        match f() {
            Ok(value) => return Ok(value),
            Err(err) if is_busy_or_locked(&err) => {
                std::thread::sleep(std::time::Duration::from_millis(*delay_ms));
            }
            Err(err) => return Err(err),
        }
    }
    f()
}

fn is_busy_or_locked(message: &str) -> bool {
    message.contains("DatabaseBusy")
        || message.contains("DatabaseLocked")
        || message.contains("database is locked")
        || message.contains("database table is locked")
}

fn delete_units(project_root: &Path, prefix_like: &str) -> Result<(), String> {
    let conn = open_index(project_root)?;
    delete_units_tx(&conn, prefix_like)
}

fn delete_units_by_scope(project_root: &Path, scope: &str) -> Result<(), String> {
    let conn = open_index(project_root)?;
    let mut stmt = conn
        .prepare("SELECT id FROM mem_items WHERE scope = ?1 AND id LIKE 'planunit_%'")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![scope], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let ids = rows.filter_map(Result::ok).collect::<Vec<_>>();
    drop(stmt);
    for id in ids {
        conn.execute(
            "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
            params![&id, format!("{}_chunk_%", id)],
        )
        .ok();
        conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM mem_items WHERE id = ?1", params![&id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn delete_units_tx(conn: &Connection, prefix_like: &str) -> Result<(), String> {
    let ids = unit_ids_like(conn, prefix_like)?;
    for id in ids {
        conn.execute(
            "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
            params![&id, format!("{}_chunk_%", id)],
        )
        .ok();
        conn.execute("DELETE FROM mem_fts WHERE mem_id = ?1", params![&id])
            .map_err(|e| e.to_string())?;
        conn.execute("DELETE FROM mem_items WHERE id = ?1", params![&id])
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn unit_ids_like(conn: &Connection, prefix_like: &str) -> Result<Vec<String>, String> {
    let mut stmt = conn
        .prepare("SELECT id FROM mem_items WHERE id LIKE ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![prefix_like], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    Ok(rows.filter_map(Result::ok).collect())
}

fn insert_unit(conn: &Connection, unit: &Unit) -> Result<(), String> {
    let now = chrono::Utc::now().to_rfc3339();
    let tags = unit_tags(unit);
    let tags_json = serde_json::to_string(&tags).map_err(|e| e.to_string())?;
    let meta = serde_json::json!({
        "source": "plan",
        "plan_id": unit.plan_id,
        "spec_id": unit.spec_id,
        "unit_id": unit.unit_id,
        "unit_title": unit.unit_title,
        "parent_title": unit.parent_title,
        "status": unit.status,
    });
    conn.execute(
        "INSERT INTO mem_items
         (id, uid, kind, scope, title, summary, area, tags_json, body_inline,
          body_blob_path, source, importance, created_at, updated_at, accessed_at,
          is_pinned_wakeup)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, 'plan', ?6, ?7, NULL, ?8, 3.0, ?9, ?9, NULL, 0)",
        params![
            &unit.id,
            &unit.id,
            unit.kind,
            &unit.scope,
            &unit.title,
            &tags_json,
            &unit.body,
            meta.to_string(),
            &now,
        ],
    )
    .map_err(|e| format!("insert plan unit: {e}"))?;
    conn.execute(
        "INSERT INTO mem_fts (mem_id, title, summary, body, tags) VALUES (?1, ?2, '', ?3, ?4)",
        params![&unit.id, &unit.title, &unit.body, tags.join(" ")],
    )
    .map_err(|e| format!("insert plan unit fts: {e}"))?;
    Ok(())
}

fn unit_tags(unit: &Unit) -> Vec<String> {
    let mut tags = vec![format!("unit:{}", unit.unit_id)];
    if let Some(plan_id) = &unit.plan_id {
        tags.push(format!("plan:{plan_id}"));
    }
    if let Some(spec_id) = &unit.spec_id {
        tags.push(format!("spec:{spec_id}"));
    }
    tags
}

fn plan_units(plan: &Plan, scope: &str) -> Vec<Unit> {
    if plan.status == PlanStatus::Archived {
        return Vec::new();
    }
    let mut units = vec![Unit {
        id: stable_id(&plan.id, "plan", &plan.id),
        kind: "plan",
        title: plan.title.clone(),
        body: format!(
            "Plan \"{}\"\n{}",
            plan.title,
            section_titles(&plan.sections)
        ),
        scope: scope.to_string(),
        plan_id: Some(plan.id.clone()),
        spec_id: None,
        unit_id: plan.id.clone(),
        unit_title: plan.title.clone(),
        parent_title: plan.title.clone(),
        status: Some("active".into()),
    }];
    let total = count_sections(&plan.sections);
    let mut ordinal = 0usize;
    collect_plan_sections(plan, &plan.sections, total, &mut ordinal, scope, &mut units);
    for task in &plan.tasks {
        units.push(task_unit(plan, task, scope));
        for stride in &task.milestones {
            units.push(stride_unit(plan, task, stride, scope));
        }
    }
    units
}

fn spec_units(spec: &Spec, scope: &str) -> Vec<Unit> {
    if spec.status == PlanStatus::Archived {
        return Vec::new();
    }
    let mut units = vec![Unit {
        id: stable_id(&spec.id, "spec", &spec.id),
        kind: "spec",
        title: spec.title.clone(),
        body: format!(
            "Spec \"{}\"\n{}",
            spec.title,
            section_titles(&spec.sections)
        ),
        scope: scope.to_string(),
        plan_id: None,
        spec_id: Some(spec.id.clone()),
        unit_id: spec.id.clone(),
        unit_title: spec.title.clone(),
        parent_title: spec.title.clone(),
        status: Some("active".into()),
    }];
    let total = count_sections(&spec.sections);
    let mut ordinal = 0usize;
    collect_spec_sections(spec, &spec.sections, total, &mut ordinal, scope, &mut units);
    units
}

fn collect_plan_sections(
    plan: &Plan,
    sections: &[Section],
    total: usize,
    ordinal: &mut usize,
    scope: &str,
    units: &mut Vec<Unit>,
) {
    for section in sections {
        *ordinal += 1;
        units.push(Unit {
            id: stable_id(&plan.id, "plan_section", &section.id),
            kind: "plan_section",
            title: format!("{} section {}", plan.title, section.title),
            body: format!(
                "Plan \"{}\" · section {}/{}: {}\n{}",
                plan.title, *ordinal, total, section.title, section.body
            ),
            scope: scope.to_string(),
            plan_id: Some(plan.id.clone()),
            spec_id: None,
            unit_id: section.id.clone(),
            unit_title: section.title.clone(),
            parent_title: plan.title.clone(),
            status: Some("active".into()),
        });
        collect_plan_sections(plan, &section.subsections, total, ordinal, scope, units);
    }
}

fn collect_spec_sections(
    spec: &Spec,
    sections: &[Section],
    total: usize,
    ordinal: &mut usize,
    scope: &str,
    units: &mut Vec<Unit>,
) {
    for section in sections {
        *ordinal += 1;
        units.push(Unit {
            id: stable_id(&spec.id, "spec_section", &section.id),
            kind: "spec_section",
            title: format!("{} section {}", spec.title, section.title),
            body: format!(
                "Spec \"{}\" · section {}/{}: {}\n{}",
                spec.title, *ordinal, total, section.title, section.body
            ),
            scope: scope.to_string(),
            plan_id: None,
            spec_id: Some(spec.id.clone()),
            unit_id: section.id.clone(),
            unit_title: section.title.clone(),
            parent_title: spec.title.clone(),
            status: Some("active".into()),
        });
        collect_spec_sections(spec, &section.subsections, total, ordinal, scope, units);
    }
}

fn task_unit(plan: &Plan, task: &Task, scope: &str) -> Unit {
    Unit {
        id: stable_id(&plan.id, "task", &task.id),
        kind: "task",
        title: format!("{} task {}", plan.title, task.name),
        body: format!(
            "Plan \"{}\" · task: {}\n{}",
            plan.title, task.name, task.body
        ),
        scope: scope.to_string(),
        plan_id: Some(plan.id.clone()),
        spec_id: None,
        unit_id: task.id.clone(),
        unit_title: task.name.clone(),
        parent_title: plan.title.clone(),
        status: Some(task_status_text(task)),
    }
}

fn stride_unit(plan: &Plan, task: &Task, stride: &Milestone, scope: &str) -> Unit {
    Unit {
        id: stable_id(&plan.id, "stride", &stride.id),
        kind: "stride",
        title: format!("{} stride {}", task.name, stride.kind),
        body: format!(
            "Plan \"{}\" · task \"{}\" · stride({}): {}",
            plan.title, task.name, stride.kind, stride.text
        ),
        scope: scope.to_string(),
        plan_id: Some(plan.id.clone()),
        spec_id: None,
        unit_id: stride.id.clone(),
        unit_title: stride.kind.clone(),
        parent_title: task.name.clone(),
        status: Some(task_status_text(task)),
    }
}

fn task_status_text(task: &Task) -> String {
    serde_json::to_value(&task.status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "pending".into())
}

fn stable_id(parent_id: &str, kind: &str, unit_id: &str) -> String {
    format!("planunit_{parent_id}_{kind}_{unit_id}")
}

fn count_sections(sections: &[Section]) -> usize {
    sections
        .iter()
        .map(|s| 1 + count_sections(&s.subsections))
        .sum::<usize>()
        .max(1)
}

fn section_titles(sections: &[Section]) -> String {
    sections
        .iter()
        .map(|s| s.title.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn search_project(
    project: &Path,
    q: &search::SearchQuery,
) -> Result<Vec<PlanSearchHit>, String> {
    let root = super::paths::project_root(project);
    let db = project_index_path(&root);
    if !db.exists() {
        return Ok(Vec::new());
    }
    let conn = open_index(&root)?;
    let blobs = root.join(".codeterm").join("plan-index-blobs");
    let mut scoped = q.clone();
    if scoped.scope.is_none() {
        scoped.scope = Some(project_scope(&root));
    }
    let hits = search::hybrid_rrf(&conn, &scoped, &blobs)?;
    Ok(hits.into_iter().map(plan_hit).collect())
}

fn plan_hit(hit: search::SearchHit) -> PlanSearchHit {
    let meta: serde_json::Value = hit
        .item
        .source
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    PlanSearchHit {
        source: "plan",
        score: hit.score,
        id: hit.item.id,
        kind: hit.item.kind,
        title: hit.item.title,
        body: hit.item.body,
        scope: hit.item.scope,
        plan_id: meta
            .get("plan_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        spec_id: meta
            .get("spec_id")
            .and_then(|v| v.as_str())
            .map(str::to_string),
        unit_id: meta
            .get("unit_id")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        unit_title: meta
            .get("unit_title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        parent_title: meta
            .get("parent_title")
            .and_then(|v| v.as_str())
            .unwrap_or_default()
            .to_string(),
        status: meta
            .get("status")
            .and_then(|v| v.as_str())
            .map(str::to_string),
    }
}

fn related_units(plans: &[Plan], specs: &[Spec], id: &str) -> Vec<RelatedGroup> {
    let mut groups = Vec::new();
    let Some(plan) = plans.iter().find(|plan| {
        plan.id == id
            || section_contains_any(&plan.sections, id)
            || plan.tasks.iter().any(|task| task.id == id)
    }) else {
        return groups;
    };

    if let Some(task) = plan.tasks.iter().find(|task| task.id == id) {
        let sections = task
            .section_ids
            .iter()
            .map(|section_id| {
                find_section(&plan.sections, section_id)
                    .map(|section| unit(section.id.clone(), "plan_section", section.title.clone()))
                    .unwrap_or_else(|| removed(section_id, "plan_section"))
            })
            .collect();
        groups.push(RelatedGroup {
            edge_type: "task_sections".into(),
            units: sections,
        });

        let children = plan
            .tasks
            .iter()
            .filter(|candidate| candidate.parent_task_id.as_deref() == Some(&task.id))
            .map(|task| unit(task.id.clone(), "task", task.name.clone()))
            .collect();
        groups.push(RelatedGroup {
            edge_type: "subtasks".into(),
            units: children,
        });
    } else if section_contains_any(&plan.sections, id) {
        let tasks = plan
            .tasks
            .iter()
            .filter(|task| task.section_ids.iter().any(|section_id| section_id == id))
            .map(|task| unit(task.id.clone(), "task", task.name.clone()))
            .collect();
        groups.push(RelatedGroup {
            edge_type: "section_tasks".into(),
            units: tasks,
        });

        let spec_links = plan
            .spec_links
            .iter()
            .filter(|link| link.plan_section_id == id)
            .map(|link| {
                specs
                    .iter()
                    .find(|spec| spec.id == link.spec_id)
                    .and_then(|spec| find_section(&spec.sections, &link.spec_section_id))
                    .map(|section| unit(section.id.clone(), "spec_section", section.title.clone()))
                    .unwrap_or_else(|| removed(&link.spec_section_id, "spec_section"))
            })
            .collect();
        groups.push(RelatedGroup {
            edge_type: "spec_links".into(),
            units: spec_links,
        });
    }

    let lineage = plan
        .lineage
        .iter()
        .map(|link| {
            plans
                .iter()
                .find(|candidate| candidate.id == link.to)
                .map(|plan| unit(plan.id.clone(), "plan", plan.title.clone()))
                .unwrap_or_else(|| removed(&link.to, "plan"))
        })
        .collect();
    groups.push(RelatedGroup {
        edge_type: "lineage".into(),
        units: lineage,
    });
    groups
}

fn unit(id: String, kind: &str, title: String) -> RelatedUnit {
    RelatedUnit {
        id,
        kind: kind.into(),
        title,
        removed: false,
    }
}

fn removed(id: &str, kind: &str) -> RelatedUnit {
    RelatedUnit {
        id: id.into(),
        kind: kind.into(),
        title: "removed".into(),
        removed: true,
    }
}

fn find_section<'a>(sections: &'a [Section], id: &str) -> Option<&'a Section> {
    for section in sections {
        if section.id == id {
            return Some(section);
        }
        if let Some(found) = find_section(&section.subsections, id) {
            return Some(found);
        }
    }
    None
}

fn section_contains(section: &Section, id: &str) -> bool {
    section.id == id || section.subsections.iter().any(|s| section_contains(s, id))
}

fn section_contains_any(sections: &[Section], id: &str) -> bool {
    sections.iter().any(|section| section_contains(section, id))
}

#[cfg(test)]
mod tests;
