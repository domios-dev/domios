use super::*;

impl super::super::PlanStore {
    pub(super) fn enqueue_project_index(
        &self,
        turn: &super::super::MutationWriterTurn<'_>,
        root: &Path,
        plans: &[Plan],
        specs: &[Spec],
    ) -> PlanReindexResult {
        turn.assert_store(self);
        let (result, jobs) = project_jobs(root, plans, specs);
        for job in jobs {
            index_queue().enqueue(job);
        }
        result
    }

    pub fn reindex_project_index(&self, project: &Path) -> Result<PlanReindexResult, String> {
        let writer_turn = self.mutation_writer.enter();
        let root = super::super::paths::project_root(project);
        Ok(self.enqueue_project_index(
            &writer_turn,
            &root,
            &self.project_plans(&root),
            &self.list_specs_full(),
        ))
    }

    /// Plans whose project scope is this root's, chosen from the stored roots and loaded through the cache.
    pub(super) fn project_plans(&self, root: &Path) -> Vec<Plan> {
        let ids = self.project_plan_ids(&project_scope(root));
        self.plans_matching(&super::super::PlanMatch::Ids(ids))
    }

    fn project_plan_ids(&self, scope: &str) -> Vec<String> {
        let mut scopes = std::collections::HashMap::<String, String>::new();
        self.plan_roots()
            .into_iter()
            .filter(|(_, root)| {
                scopes
                    .entry(root.clone())
                    .or_insert_with(|| project_scope(Path::new(root)))
                    == scope
            })
            .map(|(id, _)| id)
            .collect()
    }

    /// Builds the search index of every project that has none yet, one project at a
    /// time from SQL without filling the plan cache. Startup runs it off the open path.
    pub fn seed_missing_search_indexes(&self) {
        self.seed_missing_search_indexes_observed(|_| {});
    }

    fn seed_missing_search_indexes_observed(&self, snapshot_ready: impl Fn(&Path)) {
        let roots = self
            .plan_roots()
            .into_iter()
            .map(|(_, root)| super::super::paths::project_root(Path::new(&root)))
            .chain(
                self.list_specs_full()
                    .iter()
                    .map(|spec| super::super::paths::project_root(spec_project_root(spec))),
            )
            .collect::<std::collections::BTreeSet<_>>();
        for root in roots {
            let writer_turn = self.mutation_writer.enter();
            if project_index_path(&root).exists() {
                continue;
            }
            let ids = self.project_plan_ids(&project_scope(&root));
            let plans: Vec<Plan> = ids
                .iter()
                .filter_map(|id| match self.load_plan_from_sql(id) {
                    Ok(plan) => plan,
                    Err(error) => {
                        log::warn!("[plan.search] {error}; left out of the seeded index");
                        None
                    }
                })
                .collect();
            let specs = self.list_specs_full();
            snapshot_ready(&root);
            self.enqueue_project_index(&writer_turn, &root, &plans, &specs);
        }
    }
}

fn project_jobs(root: &Path, plans: &[Plan], specs: &[Spec]) -> (PlanReindexResult, Vec<IndexJob>) {
    let scope = project_scope(root);
    let plans = plans
        .iter()
        .filter(|plan| project_scope(plan_project_root(plan)) == scope)
        .collect::<Vec<_>>();
    let specs = specs
        .iter()
        .filter(|spec| project_scope(spec_project_root(spec)) == scope)
        .collect::<Vec<_>>();
    let ids = plans
        .iter()
        .flat_map(|plan| plan_units(plan, &scope))
        .chain(specs.iter().flat_map(|spec| spec_units(spec, &scope)))
        .map(|unit| unit.id)
        .collect::<HashSet<_>>();
    let units = ids.len();
    let mut jobs = vec![IndexJob::Prune(root.to_path_buf(), ids)];
    jobs.extend(
        plans
            .iter()
            .map(|plan| IndexJob::Plan((*plan).clone(), PlanIndexScope::Full)),
    );
    jobs.extend(specs.iter().map(|spec| IndexJob::Spec((*spec).clone())));
    (
        PlanReindexResult {
            project: root.display().to_string(),
            scope,
            plans: plans.len(),
            specs: specs.len(),
            units,
        },
        jobs,
    )
}

pub(super) fn prune_project(root: &Path, live: &HashSet<String>) -> Result<(), String> {
    let conn = open_index(root)?;
    let mut existing = indexed_units_like(&conn, "planunit_%")?;
    existing.retain(|id, _| !live.contains(id));
    apply_unit_changes(root, conn, existing, &[], false).map(drop)
}

pub(super) fn purge_plan_indexes(plan: &Plan, specs: &[Spec]) -> Result<(), String> {
    let mut indexes = HashSet::from([project_index_path(plan_project_root(plan))]);
    indexes.extend(
        specs
            .iter()
            .map(|spec| project_index_path(spec_project_root(spec))),
    );
    let directory = port().data_dir().join("plan-indexes");
    match std::fs::read_dir(&directory) {
        Ok(entries) => {
            for entry in entries {
                let entry = entry.map_err(|error| format!("read plan index directory: {error}"))?;
                let db = entry.path().join(INDEX_FILE);
                if db.is_file() {
                    indexes.insert(db);
                }
            }
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(format!("read plan indexes: {error}")),
    }
    let entities = std::iter::once(plan.id.as_str())
        .chain(specs.iter().map(|spec| spec.id.as_str()))
        .collect::<HashSet<_>>();
    purge_indexes(
        &plan.id,
        indexes.into_iter().filter(|db| db.is_file()),
        |db| purge_entities(db, &entities),
    )
}

fn purge_indexes(
    plan_id: &str,
    indexes: impl IntoIterator<Item = PathBuf>,
    mut purge: impl FnMut(&Path) -> Result<(), String>,
) -> Result<(), String> {
    for db in indexes {
        purge(&db)
            .map_err(|error| format!("delete plan {plan_id} from {}: {error}", db.display()))?;
    }
    Ok(())
}

fn owns_unit(entities: &HashSet<&str>, id: &str) -> bool {
    entities
        .iter()
        .any(|entity| id.starts_with(&format!("planunit_{entity}_")))
}

fn purge_entities(db: &Path, entities: &HashSet<&str>) -> Result<(), String> {
    port().ensure_embedding_registered();
    let mut conn = Connection::open(db).map_err(|error| error.to_string())?;
    conn.busy_timeout(PLAN_INDEX_BUSY_TIMEOUT)
        .map_err(|error| error.to_string())?;
    port().init_embedding_ext(&conn);
    let units = indexed_units_like(&conn, "planunit_%")?;
    let vectors: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name = 'mem_vectors')",
            [],
            |row| row.get(0),
        )
        .map_err(|error| error.to_string())?;
    let tx = conn.transaction().map_err(|error| error.to_string())?;
    if vectors {
        for entity in entities {
            tx.execute(
                "DELETE FROM mem_vectors WHERE mem_id LIKE ?1",
                [format!("planunit_{entity}_%")],
            )
            .map_err(|error| error.to_string())?;
        }
    }
    for id in units.keys().filter(|id| owns_unit(entities, id)) {
        tx.execute("DELETE FROM mem_fts WHERE mem_id = ?1", [id])
            .map_err(|error| error.to_string())?;
        tx.execute("DELETE FROM mem_items WHERE id = ?1", [id])
            .map_err(|error| error.to_string())?;
    }
    tx.commit().map_err(|error| error.to_string())
}

#[cfg(test)]
mod seed_tests;
#[cfg(test)]
mod tests;
