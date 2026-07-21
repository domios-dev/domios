use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock};

use parking_lot::{Condvar, Mutex};
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

fn port() -> Arc<dyn PlanSearchPort> {
    SEARCH_PORT
        .get()
        .expect("search port not initialized; call init_search_port before using search_index")
        .clone()
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

enum IndexJob {
    Plan(Plan),
    DeletePlan(Plan),
    Spec(Spec),
}

impl IndexJob {
    fn key(&self) -> String {
        match self {
            Self::Plan(plan) | Self::DeletePlan(plan) => {
                format!("{}:plan:{}", self.project_key(), plan.id)
            }
            Self::Spec(spec) => format!("{}:spec:{}", self.project_key(), spec.id),
        }
    }

    fn project_key(&self) -> String {
        match self {
            Self::Plan(plan) | Self::DeletePlan(plan) => project_scope(plan_project_root(plan)),
            Self::Spec(spec) => project_scope(spec_project_root(spec)),
        }
    }
}

#[derive(Default)]
struct IndexQueueState {
    order: VecDeque<String>,
    latest: HashMap<String, IndexJob>,
    in_flight_keys: HashSet<String>,
    in_flight_projects: HashSet<String>,
    in_flight: usize,
}

struct IndexQueue {
    state: Mutex<IndexQueueState>,
    ready: Condvar,
}

impl IndexQueue {
    fn enqueue(&self, job: IndexJob) {
        let key = job.key();
        let mut state = self.state.lock();
        if state.latest.insert(key.clone(), job).is_none() {
            state.order.push_back(key);
        }
        self.ready.notify_one();
    }

    fn enqueue_and_wait(&self, job: IndexJob) {
        let key = job.key();
        self.enqueue(job);
        let mut state = self.state.lock();
        while state.latest.contains_key(&key) || state.in_flight_keys.contains(&key) {
            self.ready.wait(&mut state);
        }
    }

    fn take(&self) -> (String, String, IndexJob) {
        let mut state = self.state.lock();
        while state.order.iter().all(|key| {
            state.in_flight_keys.contains(key)
                || state
                    .latest
                    .get(key)
                    .is_some_and(|job| state.in_flight_projects.contains(&job.project_key()))
        }) {
            self.ready.wait(&mut state);
        }
        let position = state
            .order
            .iter()
            .position(|key| {
                !state.in_flight_keys.contains(key)
                    && state
                        .latest
                        .get(key)
                        .is_some_and(|job| !state.in_flight_projects.contains(&job.project_key()))
            })
            .expect("runnable index job");
        let key = state.order.remove(position).expect("queued index job");
        let job = state.latest.remove(&key).expect("queued index job");
        let project = job.project_key();
        state.in_flight_keys.insert(key.clone());
        state.in_flight_projects.insert(project.clone());
        state.in_flight += 1;
        (key, project, job)
    }

    fn complete(&self, key: String, project: String) {
        let mut state = self.state.lock();
        state.in_flight_keys.remove(&key);
        state.in_flight_projects.remove(&project);
        state.in_flight -= 1;
        self.ready.notify_all();
    }

    #[cfg(test)]
    fn wait_until_project_idle(&self, project_root: &Path) {
        let project = project_scope(project_root);
        let mut state = self.state.lock();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while state.in_flight_projects.contains(&project)
            || state
                .latest
                .values()
                .any(|job| job.project_key() == project)
        {
            let remaining = deadline.saturating_duration_since(std::time::Instant::now());
            assert!(!remaining.is_zero(), "plan index queue did not drain");
            self.ready.wait_for(&mut state, remaining);
        }
    }
}

fn index_queue() -> &'static Arc<IndexQueue> {
    static QUEUE: OnceLock<Arc<IndexQueue>> = OnceLock::new();
    QUEUE.get_or_init(|| {
        let queue = Arc::new(IndexQueue {
            state: Mutex::new(IndexQueueState::default()),
            ready: Condvar::new(),
        });
        for worker in 0..if cfg!(test) { 16 } else { 1 } {
            let worker_queue = queue.clone();
            std::thread::Builder::new()
                .name(format!("plan-index-queue-{worker}"))
                .spawn(move || loop {
                    let (key, project, job) = worker_queue.take();
                    match job {
                        IndexJob::Plan(plan) => {
                            if let Err(error) = mirror_plan(&plan, true) {
                                log::warn!("[plan.index] plan {}: {}", plan.id, error);
                            }
                        }
                        IndexJob::DeletePlan(plan) => {
                            if let Err(error) = delete_plan_units(&plan) {
                                log::warn!("[plan.index] delete plan {}: {}", plan.id, error);
                            }
                        }
                        IndexJob::Spec(spec) => {
                            if let Err(error) = mirror_spec(&spec, true) {
                                log::warn!("[plan.index] spec {}: {}", spec.id, error);
                            }
                        }
                    }
                    worker_queue.complete(key, project);
                })
                .expect("spawn plan index queue");
        }
        queue
    })
}

pub(super) fn enqueue_plan_index(plan: Plan) {
    index_queue().enqueue(IndexJob::Plan(plan));
}

pub(super) fn enqueue_spec_index(spec: Spec) {
    index_queue().enqueue(IndexJob::Spec(spec));
}

pub(super) fn remove_plan_from_index(plan: Plan) {
    index_queue().enqueue_and_wait(IndexJob::DeletePlan(plan));
}

#[cfg(test)]
pub(crate) fn flush_index_queue_for_test(project_root: &Path) {
    index_queue().wait_until_project_idle(project_root);
}

impl super::PlanStore {
    pub fn mirror_plan_index(&self, plan: &Plan) -> Result<(), String> {
        enqueue_plan_index(plan.clone());
        Ok(())
    }

    pub fn mirror_spec_index(&self, spec: &Spec) -> Result<(), String> {
        enqueue_spec_index(spec.clone());
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
        let _writer_turn = self.mutation_writer.enter();
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
        let units = plans
            .iter()
            .map(|plan| plan_units(plan, &project_scope(&root)).len())
            .sum::<usize>()
            + specs
                .iter()
                .map(|spec| spec_units(spec, &project_scope(&root)).len())
                .sum::<usize>();
        for plan in &plans {
            self.database
                .persist_plan(plan, None)
                .map_err(|error| error.to_string())?;
            enqueue_plan_index(plan.clone());
        }
        for spec in &specs {
            self.database
                .persist_spec(spec)
                .map_err(|error| error.to_string())?;
            enqueue_spec_index(spec.clone());
        }
        Ok(PlanReindexResult {
            project: root_text,
            scope: project_scope(&root),
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

pub fn project_index_path(project_root: &Path) -> PathBuf {
    let root = super::paths::project_root(project_root);
    project_index_dir(&root).join(INDEX_FILE)
}

pub fn project_index_dir(project_root: &Path) -> PathBuf {
    let root = super::paths::project_root(project_root);
    #[cfg(test)]
    if super::paths::test_project_root(&root).is_some() {
        return root.join(".codeterm").join("test-plan-index");
    }
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

pub fn delete_plan_units(plan: &Plan) -> Result<(), String> {
    delete_units(plan_project_root(plan), &format!("planunit_{}_%", plan.id))
}

fn open_index(project_root: &Path) -> Result<Connection, String> {
    open_index_with_timeout(project_root, PLAN_INDEX_BUSY_TIMEOUT, true)
}

fn open_search_index(project_root: &Path) -> Result<Connection, String> {
    open_index_with_timeout(project_root, PLAN_INDEX_SEARCH_TIMEOUT, true)
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
    let existing = indexed_units_like(&conn, prefix_like)?;
    let next_ids = units
        .iter()
        .map(|unit| unit.id.clone())
        .collect::<HashSet<_>>();
    let row_changes = units
        .iter()
        .filter(|unit| existing.get(&unit.id) != Some(&unit_signature(unit)))
        .cloned()
        .collect::<Vec<_>>();
    let embedding_changes = row_changes
        .iter()
        .filter(|unit| {
            existing
                .get(&unit.id)
                .map(|signature| signature.body != unit.body || signature.title != unit.title)
                .unwrap_or(true)
        })
        .cloned()
        .collect::<Vec<_>>();
    let stale_vectors = existing
        .keys()
        .filter(|id| !next_ids.contains(*id))
        .chain(embedding_changes.iter().map(|unit| &unit.id))
        .cloned()
        .collect::<HashSet<_>>();
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    delete_changed_unit_rows_tx(&tx, &stale_vectors, &row_changes, &existing)?;
    for unit in &row_changes {
        insert_unit(&tx, unit)?;
    }
    tx.commit().map_err(|e| e.to_string())?;
    if sync_embed {
        let conn = open_index(project_root)?;
        for unit in &embedding_changes {
            let _ = port().embed(&conn, &unit.id, &unit.body);
        }
    }
    Ok(embedding_changes)
}

fn delete_units(project_root: &Path, prefix_like: &str) -> Result<(), String> {
    let conn = open_index(project_root)?;
    delete_units_tx(&conn, prefix_like)
}

#[cfg(test)]
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct IndexedUnit {
    title: String,
    body: String,
    kind: String,
    scope: String,
    source: String,
    updated_at: String,
}

fn unit_signature(unit: &Unit) -> IndexedUnit {
    IndexedUnit {
        title: unit.title.clone(),
        body: unit.body.clone(),
        kind: unit.kind.to_string(),
        scope: unit.scope.clone(),
        source: serde_json::json!({
            "source": "plan",
            "plan_id": unit.plan_id,
            "spec_id": unit.spec_id,
            "unit_id": unit.unit_id,
            "unit_title": unit.unit_title,
            "parent_title": unit.parent_title,
            "status": unit.status,
            "updated_at": unit.updated_at,
        })
        .to_string(),
        updated_at: timestamp_ms_to_rfc3339(unit.updated_at),
    }
}

fn delete_changed_unit_rows_tx(
    conn: &Connection,
    stale_vectors: &HashSet<String>,
    row_changes: &[Unit],
    existing: &HashMap<String, IndexedUnit>,
) -> Result<(), String> {
    let changed_ids = row_changes
        .iter()
        .map(|unit| unit.id.clone())
        .collect::<HashSet<_>>();
    for id in existing.keys() {
        if !stale_vectors.contains(id) && !changed_ids.contains(id) {
            continue;
        }
        if stale_vectors.contains(id) {
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

fn indexed_units_like(
    conn: &Connection,
    prefix_like: &str,
) -> Result<HashMap<String, IndexedUnit>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, title, body_inline, kind, scope, source, updated_at
             FROM mem_items WHERE id LIKE ?1",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map(params![prefix_like], |r| {
            Ok((
                r.get::<_, String>(0)?,
                IndexedUnit {
                    title: r.get(1)?,
                    body: r.get::<_, Option<String>>(2)?.unwrap_or_default(),
                    kind: r.get(3)?,
                    scope: r.get(4)?,
                    source: r.get(5)?,
                    updated_at: r.get(6)?,
                },
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut bodies = HashMap::new();
    for row in rows {
        let (id, unit) = row.map_err(|e| e.to_string())?;
        bodies.insert(id, unit);
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
