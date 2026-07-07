use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, OnceLock};

use rusqlite::{params, Connection};
use serde::Serialize;
use sha2::{Digest, Sha256};

use super::ports::{PlanIndexHit, PlanIndexQuery, PlanSearchPort};
use crate::plan::{Milestone, Plan, PlanStatus, Section, Spec, Task};

pub use super::search_related::{RelatedGroup, RelatedUnit};

static SEARCH_PORT: OnceLock<Arc<dyn PlanSearchPort>> = OnceLock::new();

pub fn init_search_port(port: Arc<dyn PlanSearchPort>) {
    SEARCH_PORT.get_or_init(|| port);
}

fn port() -> &'static Arc<dyn PlanSearchPort> {
    SEARCH_PORT
        .get()
        .expect("search port not initialized; call init_search_port before using search_index")
}

const INDEX_FILE: &str = "plan-index.sqlite";
#[cfg(not(test))]
const PLAN_INDEX_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);
#[cfg(test)]
const PLAN_INDEX_BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(75);
const PLAN_INDEX_SEARCH_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(50);

#[derive(Debug, Clone, Serialize)]
pub struct PlanSearchHit {
    pub source: &'static str,
    pub score: f64,
    pub id: String,
    pub kind: String,
    pub title: String,
    pub body: String,
    pub scope: String,
    pub updated_at: u64,
    pub age_ms: u64,
    pub plan_id: Option<String>,
    pub spec_id: Option<String>,
    pub unit_id: String,
    pub unit_title: String,
    pub parent_title: String,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlanSearchStatus {
    Active,
    Archived,
    All,
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
    updated_at: u64,
}

impl super::PlanStore {
    pub fn mirror_plan_index(&self, plan: &Plan) -> Result<(), String> {
        let sync_embed = cfg!(test);
        let changed = mirror_plan(plan, sync_embed)?;
        if !sync_embed {
            index_units_vectors_async(plan_project_root(plan).to_path_buf(), changed);
        }
        Ok(())
    }

    pub fn mirror_spec_index(&self, spec: &Spec) -> Result<(), String> {
        let sync_embed = cfg!(test);
        let changed = mirror_spec(spec, sync_embed)?;
        if !sync_embed {
            index_units_vectors_async(spec_project_root(spec).to_path_buf(), changed);
        }
        Ok(())
    }

    pub fn search_plan_index(
        &self,
        project: &Path,
        q: &PlanIndexQuery,
    ) -> Result<Vec<PlanSearchHit>, String> {
        search_project(project, q, PlanSearchStatus::Active)
    }

    pub fn search_plan_index_with_status(
        &self,
        project: &Path,
        q: &PlanIndexQuery,
        status: PlanSearchStatus,
    ) -> Result<Vec<PlanSearchHit>, String> {
        search_project(project, q, status)
    }

    pub fn reindex_project_index(&self, project: &Path) -> Result<PlanReindexResult, String> {
        let root = super::paths::project_root(project);
        let root_text = root.display().to_string();
        let plans = self
            .list_full()
            .into_iter()
            .filter(|plan| plan_project_root(plan).display().to_string() == root_text)
            .collect::<Vec<_>>();
        let specs = self
            .list_specs_full()
            .into_iter()
            .filter(|spec| spec_project_root(spec).display().to_string() == root_text)
            .collect::<Vec<_>>();
        let scope = project_scope(&root);
        delete_units_by_scope(&root, &scope)?;
        let mut units = 0usize;
        for plan in &plans {
            units += plan_units(plan, &scope).len();
            mirror_plan(plan, true)?;
        }
        for spec in &specs {
            units += spec_units(spec, &scope).len();
            mirror_spec(spec, true)?;
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
        super::search_related::related_units(&self.list_full(), &self.list_specs_full(), id)
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
    let root = super::paths::project_root(project_root);
    project_index_dir(&root).join(INDEX_FILE)
}

pub fn project_index_dir(project_root: &Path) -> PathBuf {
    let root = super::paths::project_root(project_root);
    port()
        .data_dir()
        .join("plan-indexes")
        .join(project_scope(&root))
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

fn mirror_plan(plan: &Plan, sync_embed: bool) -> Result<Vec<Unit>, String> {
    let root = plan_project_root(plan);
    let units = plan_units(plan, &project_scope(root));
    replace_units(root, &format!("planunit_{}_%", plan.id), units, sync_embed)
}

fn mirror_spec(spec: &Spec, sync_embed: bool) -> Result<Vec<Unit>, String> {
    let root = spec_project_root(spec);
    let units = spec_units(spec, &project_scope(root));
    replace_units(root, &format!("planunit_{}_%", spec.id), units, sync_embed)
}

fn plan_project_root(plan: &Plan) -> &Path {
    if plan.canonical_project_path.trim().is_empty() {
        Path::new(&plan.project_path)
    } else {
        Path::new(&plan.canonical_project_path)
    }
}

fn spec_project_root(spec: &Spec) -> &Path {
    if spec.canonical_project_path.trim().is_empty() {
        Path::new(&spec.project_path)
    } else {
        Path::new(&spec.canonical_project_path)
    }
}

fn index_units_vectors_async(root: PathBuf, units: Vec<Unit>) {
    if units.is_empty() {
        return;
    }
    if let Err(err) = vector_index_sender().send((root, units)) {
        log::warn!("[plan.index] vector mirror enqueue failed: {}", err);
    }
}

fn vector_index_sender() -> &'static mpsc::Sender<(PathBuf, Vec<Unit>)> {
    static SENDER: OnceLock<mpsc::Sender<(PathBuf, Vec<Unit>)>> = OnceLock::new();
    SENDER.get_or_init(|| {
        let (tx, rx) = mpsc::channel::<(PathBuf, Vec<Unit>)>();
        std::thread::spawn(move || {
            for (root, units) in rx {
                index_units_vectors(root, units);
            }
        });
        tx
    })
}

fn index_units_vectors(root: PathBuf, units: Vec<Unit>) {
    let Ok(conn) = open_index(&root) else {
        return;
    };
    for unit in units {
        if let Err(err) = port().embed(&conn, &unit.id, &unit.body) {
            log::warn!(
                "[plan.index] vector mirror skipped for {}: {}",
                unit.id,
                err
            );
            continue;
        }
        if !unit_still_current(&conn, &unit) {
            delete_vectors_for_unit(&conn, &unit.id);
        }
    }
}

fn unit_still_current(conn: &Connection, unit: &Unit) -> bool {
    conn.query_row(
        "SELECT body_inline FROM mem_items WHERE id = ?1",
        params![&unit.id],
        |row| row.get::<_, String>(0),
    )
    .map(|body| body == unit.body)
    .unwrap_or(false)
}

fn delete_vectors_for_unit(conn: &Connection, unit_id: &str) {
    let _ = conn.execute(
        "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
        params![unit_id, format!("{}_chunk_%", unit_id)],
    );
}

pub fn delete_plan_units(plan: &Plan) -> Result<(), String> {
    delete_units(plan_project_root(plan), &format!("planunit_{}_%", plan.id))
}

fn open_index(project_root: &Path) -> Result<Connection, String> {
    open_index_with_timeout(project_root, PLAN_INDEX_BUSY_TIMEOUT, true)
}

fn open_search_index(project_root: &Path) -> Result<Connection, String> {
    open_index_with_timeout(project_root, PLAN_INDEX_SEARCH_TIMEOUT, false)
}

fn open_index_with_timeout(
    project_root: &Path,
    timeout: std::time::Duration,
    init_schema: bool,
) -> Result<Connection, String> {
    let db_path = project_index_path(project_root);
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create plan index dir: {e}"))?;
    }
    port().ensure_embedding_registered();
    let conn = Connection::open(&db_path).map_err(|e| format!("open plan index: {e}"))?;
    conn.busy_timeout(timeout)
        .map_err(|e| format!("set plan index busy_timeout: {e}"))?;
    if init_schema {
        port().init_schema(&conn)?;
    }
    port().init_embedding_ext(&conn);
    Ok(conn)
}

fn replace_units(
    project_root: &Path,
    prefix_like: &str,
    units: Vec<Unit>,
    sync_embed: bool,
) -> Result<Vec<Unit>, String> {
    replace_units_once(project_root, prefix_like, &units, sync_embed)
}

fn replace_units_once(
    project_root: &Path,
    prefix_like: &str,
    units: &[Unit],
    sync_embed: bool,
) -> Result<Vec<Unit>, String> {
    let mut conn = open_index(project_root)?;
    let existing = unit_bodies_like(&conn, prefix_like)?;
    let next_ids = units
        .iter()
        .map(|unit| unit.id.clone())
        .collect::<HashSet<_>>();
    let changed = units
        .iter()
        .filter(|unit| existing.get(&unit.id) != Some(&unit.body))
        .cloned()
        .collect::<Vec<_>>();
    let stale_vectors = existing
        .keys()
        .filter(|id| !next_ids.contains(*id))
        .chain(changed.iter().map(|unit| &unit.id))
        .cloned()
        .collect::<HashSet<_>>();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    replace_unit_rows_tx(&tx, prefix_like, &stale_vectors)?;
    for unit in units {
        insert_unit(&tx, unit)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    if sync_embed {
        let conn = open_index(project_root)?;
        for unit in &changed {
            let _ = port().embed(&conn, &unit.id, &unit.body);
        }
    }
    Ok(changed)
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

fn replace_unit_rows_tx(
    conn: &Connection,
    prefix_like: &str,
    stale_vectors: &HashSet<String>,
) -> Result<(), String> {
    let ids = unit_ids_like(conn, prefix_like)?;
    for id in ids {
        if stale_vectors.contains(&id) {
            conn.execute(
                "DELETE FROM mem_vectors WHERE mem_id = ?1 OR mem_id LIKE ?2",
                params![&id, format!("{}_chunk_%", id)],
            )
            .ok();
        }
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

fn unit_bodies_like(
    conn: &Connection,
    prefix_like: &str,
) -> Result<HashMap<String, String>, String> {
    let mut stmt = conn
        .prepare("SELECT id, body_inline FROM mem_items WHERE id LIKE ?1")
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![prefix_like], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| e.to_string())?;
    let mut bodies = HashMap::new();
    for row in rows {
        let (id, body) = row.map_err(|e| e.to_string())?;
        bodies.insert(id, body.unwrap_or_default());
    }
    Ok(bodies)
}

fn insert_unit(conn: &Connection, unit: &Unit) -> Result<(), String> {
    let updated_at = timestamp_ms_to_rfc3339(unit.updated_at);
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
        "updated_at": unit.updated_at,
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
            &updated_at,
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
    let plan_status = plan_status_text(&plan.status);
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
        status: Some(plan_status.clone()),
        updated_at: plan.updated_at,
    }];
    let total = count_sections(&plan.sections);
    let mut ordinal = 0usize;
    collect_plan_sections(
        plan,
        &plan.sections,
        total,
        &mut ordinal,
        scope,
        &plan_status,
        &mut units,
    );
    for task in &plan.tasks {
        units.push(task_unit(plan, task, scope, &plan_status));
        for stride in &task.milestones {
            units.push(stride_unit(plan, task, stride, scope, &plan_status));
        }
    }
    units
}

fn spec_units(spec: &Spec, scope: &str) -> Vec<Unit> {
    let spec_status = plan_status_text(&spec.status);
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
        status: Some(spec_status.clone()),
        updated_at: spec.updated_at,
    }];
    let total = count_sections(&spec.sections);
    let mut ordinal = 0usize;
    collect_spec_sections(
        spec,
        &spec.sections,
        total,
        &mut ordinal,
        scope,
        &spec_status,
        &mut units,
    );
    units
}

fn collect_plan_sections(
    plan: &Plan,
    sections: &[Section],
    total: usize,
    ordinal: &mut usize,
    scope: &str,
    status: &str,
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
            status: Some(status.to_string()),
            updated_at: plan.updated_at,
        });
        collect_plan_sections(
            plan,
            &section.subsections,
            total,
            ordinal,
            scope,
            status,
            units,
        );
    }
}

fn collect_spec_sections(
    spec: &Spec,
    sections: &[Section],
    total: usize,
    ordinal: &mut usize,
    scope: &str,
    status: &str,
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
            status: Some(status.to_string()),
            updated_at: spec.updated_at,
        });
        collect_spec_sections(
            spec,
            &section.subsections,
            total,
            ordinal,
            scope,
            status,
            units,
        );
    }
}

fn task_unit(plan: &Plan, task: &Task, scope: &str, plan_status: &str) -> Unit {
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
        status: Some(format!("{plan_status}:{}", task_status_text(task))),
        updated_at: task.updated_at,
    }
}

fn stride_unit(
    plan: &Plan,
    task: &Task,
    stride: &Milestone,
    scope: &str,
    plan_status: &str,
) -> Unit {
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
        status: Some(format!("{plan_status}:{}", task_status_text(task))),
        updated_at: stride.ts,
    }
}

fn timestamp_ms_to_rfc3339(ms: u64) -> String {
    chrono::DateTime::<chrono::Utc>::from_timestamp_millis(ms as i64)
        .unwrap_or_else(chrono::Utc::now)
        .to_rfc3339()
}

fn task_status_text(task: &Task) -> String {
    serde_json::to_value(&task.status)
        .ok()
        .and_then(|v| v.as_str().map(str::to_string))
        .unwrap_or_else(|| "pending".into())
}

fn plan_status_text(status: &PlanStatus) -> String {
    match status {
        PlanStatus::Active => "active".into(),
        PlanStatus::Archived => "archived".into(),
    }
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
    q: &PlanIndexQuery,
    status: PlanSearchStatus,
) -> Result<Vec<PlanSearchHit>, String> {
    let root = super::paths::project_root(project);
    let db = project_index_path(&root);
    if !db.exists() {
        return Ok(Vec::new());
    }
    let conn = open_search_index(&root)?;
    let blobs = project_index_dir(&root).join("blobs");
    let mut scoped = q.clone();
    if scoped.scope.is_none() {
        scoped.scope = Some(project_scope(&root));
    }
    let requested_limit = scoped.limit;
    if !matches!(status, PlanSearchStatus::All) {
        let limit = requested_limit.unwrap_or(5).clamp(1, 50);
        scoped.limit = Some((limit * 5).clamp(limit, 50));
    }
    let hits = port().run_search(&conn, &scoped, &blobs)?;
    let mut hits = hits
        .into_iter()
        .map(plan_hit)
        .filter(|hit| plan_hit_matches_status(hit, status))
        .collect::<Vec<_>>();
    if let Some(limit) = requested_limit {
        hits.truncate(limit);
    }
    Ok(hits)
}

fn plan_hit_matches_status(hit: &PlanSearchHit, status: PlanSearchStatus) -> bool {
    match status {
        PlanSearchStatus::All => true,
        PlanSearchStatus::Active => hit
            .status
            .as_deref()
            .is_some_and(|value| value == "active" || value.starts_with("active:")),
        PlanSearchStatus::Archived => hit
            .status
            .as_deref()
            .is_some_and(|value| value == "archived" || value.starts_with("archived:")),
    }
}

fn plan_hit(hit: PlanIndexHit) -> PlanSearchHit {
    let meta: serde_json::Value = hit
        .source
        .as_deref()
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or_default();
    let updated_at = meta
        .get("updated_at")
        .and_then(|v| v.as_u64())
        .or_else(|| rfc3339_to_timestamp_ms(&hit.updated_at))
        .unwrap_or_default();
    PlanSearchHit {
        source: "plan",
        score: hit.score,
        id: hit.id,
        kind: hit.kind,
        title: hit.title,
        body: hit.body,
        scope: hit.scope,
        updated_at,
        age_ms: u64::try_from(chrono::Utc::now().timestamp_millis())
            .unwrap_or_default()
            .saturating_sub(updated_at),
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

fn rfc3339_to_timestamp_ms(value: &str) -> Option<u64> {
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .and_then(|dt| u64::try_from(dt.timestamp_millis()).ok())
}

#[cfg(test)]
mod tests;
