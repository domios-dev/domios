use super::mutations::{
    has_section, push_subsection, rm_section_tree, update_section_fields, update_section_tags,
};
use super::{PlanError, PlanStore};
use crate::plan::{EffortLink, EffortLinkRel, PlanStatus, Section, Spec, SpecLink};

fn gen_id(prefix: &str) -> String {
    crate::plan::ids::short_id(prefix)
}

impl PlanStore {
    #[cfg(test)]
    pub fn create_plan_from_spec(
        &self,
        spec_id: &str,
        title: String,
        owner: crate::plan::OrchestratorRef,
        kinds: Vec<crate::plan::MilestoneKind>,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, PlanError> {
        let team_id = self.mint_id("team_test");
        self.create_plan_from_spec_for_team(spec_id, title, owner, team_id, kinds, now_ms)
    }

    pub fn create_plan_from_spec_for_team(
        &self,
        spec_id: &str,
        title: String,
        owner: crate::plan::OrchestratorRef,
        team_id: String,
        kinds: Vec<crate::plan::MilestoneKind>,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, PlanError> {
        self.create_plan_from_spec_for_team_with_planner(
            spec_id, title, owner, None, team_id, kinds, now_ms,
        )
    }

    pub fn create_plan_from_spec_for_team_with_planner(
        &self,
        spec_id: &str,
        title: String,
        owner: crate::plan::OrchestratorRef,
        planner: Option<crate::plan::PlannerRef>,
        team_id: String,
        kinds: Vec<crate::plan::MilestoneKind>,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, PlanError> {
        let spec = self.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        // Claim ownership before inserting the plan row: a spec-conflict must
        // leave zero orphan plans behind.
        if let Some(owner) = spec.plan_id.as_deref() {
            return Err(PlanError::Io(format!(
                "spec {spec_id} already owned by plan {owner}"
            )));
        }
        let mut plan = match planner {
            Some(planner) => self.create_first_plan_for_team_with_planner(
                title,
                spec.project_path.clone(),
                owner,
                planner,
                team_id,
                kinds,
                now_ms,
            )?,
            None => self.create_plan_for_team(
                title,
                spec.project_path.clone(),
                owner,
                team_id,
                kinds,
                now_ms,
            )?,
        };
        plan.primary_spec_id = Some(spec_id.to_string());
        if !plan.spec_refs.iter().any(|id| id == spec_id) {
            plan.spec_refs.push(spec_id.to_string());
        }

        fn mirror_sections(
            source: &[Section],
            links: &mut Vec<SpecLink>,
            spec_id: &str,
        ) -> Vec<Section> {
            source
                .iter()
                .map(|spec_section| {
                    let mirrored_id = gen_id("sec");
                    let mirrored_subsections =
                        mirror_sections(&spec_section.subsections, links, spec_id);
                    links.push(SpecLink {
                        spec_id: spec_id.to_string(),
                        spec_section_id: spec_section.id.clone(),
                        plan_section_id: mirrored_id.clone(),
                    });
                    Section {
                        id: mirrored_id,
                        title: spec_section.title.clone(),
                        handle: None,
                        tags: vec![],
                        body: String::new(),
                        subsections: mirrored_subsections,
                        canvas: Default::default(),
                        attachments: vec![],
                    }
                })
                .collect()
        }

        let mut links = Vec::new();
        plan.sections = mirror_sections(&spec.sections, &mut links, spec_id);
        plan.spec_links = links;
        self.commit_loaded_plan(plan.clone())?;
        // Roll back the just-committed plan if the claim races another owner,
        // so no orphan survives the failure.
        if let Err(e) = self.adopt_spec_to_plan(spec_id, &plan.id, now_ms) {
            let _ = self.delete(&plan.id);
            return Err(e);
        }
        Ok(plan)
    }

    /// Bind a spec to its owning plan (1:1). Persisting it then co-locates the
    /// spec.yaml into the plan dir and drops the orphan global copy.
    fn adopt_spec_to_plan(
        &self,
        spec_id: &str,
        plan_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_spec(spec_id, now_ms, |spec| {
            if let Some(owner) = spec.plan_id.as_deref() {
                if owner != plan_id {
                    return Err(PlanError::Io(format!(
                        "spec {spec_id} already owned by plan {owner}"
                    )));
                }
            }
            spec.plan_id = Some(plan_id.to_string());
            Ok(())
        })
    }

    pub fn create_spec(
        &self,
        title: String,
        project_path: String,
        now_ms: u64,
    ) -> Result<Spec, PlanError> {
        let project_path =
            crate::service_plan::paths::project_root(std::path::Path::new(&project_path))
                .display()
                .to_string();
        let mut spec = Spec::new(title, project_path);
        if self.id_taken(&spec.id) {
            spec.id = self.mint_id("sp");
        }
        self.save_spec(&mut spec, now_ms)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        Ok(spec)
    }

    pub fn update_spec(
        &self,
        spec_id: &str,
        title: Option<&str>,
        status: Option<PlanStatus>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_spec(spec_id, now_ms, |spec| {
            if let Some(title) = title {
                spec.title = title.into();
            }
            if let Some(status) = status {
                spec.status = status;
            }
            Ok(())
        })
    }

    pub(super) fn with_spec<T, F>(&self, id: &str, now_ms: u64, f: F) -> Result<T, PlanError>
    where
        F: FnOnce(&mut Spec) -> Result<T, PlanError>,
    {
        let arc = self
            .specs
            .read()
            .get(id)
            .cloned()
            .ok_or(PlanError::NotFound)?;
        let mut guard = arc.lock();
        let out = f(&mut guard)?;
        guard.updated_at = now_ms;
        if guard.canonical_project_path.trim().is_empty() {
            guard.canonical_project_path = guard.project_path.clone();
        }
        super::tag_model::normalize_sections_for_spec(&mut guard.sections);
        let path = self
            .persist_spec(&guard)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_spec_write(&guard, &path);
        let entry = Self::spec_index_entry(&guard, path);
        self.spec_index.write().insert(guard.id.clone(), entry);
        Ok(out)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn add_spec_section(
        &self,
        spec_id: &str,
        title: &str,
        body: &str,
        parent: Option<&str>,
        tags: &[String],
        primary: Option<&str>,
        handle: Option<&str>,
        now_ms: u64,
    ) -> Result<String, PlanError> {
        let sid = self.mint_id("sec");
        let mut section_tags = tags.to_vec();
        super::tag_model::apply_tags(&mut section_tags, &[], primary);
        let section = Section {
            id: sid.clone(),
            title: title.into(),
            handle: handle.map(str::to_string),
            tags: section_tags,
            body: body.into(),
            subsections: vec![],
            canvas: Default::default(),
            attachments: vec![],
        };
        self.with_spec(spec_id, now_ms, |spec| match parent {
            None => {
                spec.sections.push(section);
                Ok(())
            }
            Some(pid) => match push_subsection(&mut spec.sections, pid, section) {
                None => Ok(()),
                Some(_) => Err(PlanError::NotFound),
            },
        })?;
        Ok(sid)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_spec_section(
        &self,
        spec_id: &str,
        section_id: &str,
        title: Option<&str>,
        body: Option<&str>,
        tags: &[String],
        primary: Option<&str>,
        handle: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_spec(spec_id, now_ms, |spec| {
            if !update_section_fields(&mut spec.sections, section_id, title, body) {
                return Err(PlanError::NotFound);
            }
            update_section_tags(&mut spec.sections, section_id, tags, primary, handle);
            Ok(())
        })
    }

    pub fn rm_spec_section(
        &self,
        spec_id: &str,
        section_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_spec(spec_id, now_ms, |spec| {
            rm_section_tree(&mut spec.sections, section_id)
                .then_some(())
                .ok_or(PlanError::NotFound)
        })
    }

    pub fn link_spec_section(
        &self,
        plan_id: &str,
        spec_id: &str,
        spec_section_id: &str,
        plan_section_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        let spec = self.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        if !has_section(&spec.sections, spec_section_id) {
            return Err(PlanError::NotFound);
        }
        self.with_plan(plan_id, now_ms, |plan| {
            if !has_section(&plan.sections, plan_section_id) {
                return Err(PlanError::NotFound);
            }
            if !plan.spec_refs.iter().any(|id| id == spec_id) {
                plan.spec_refs.push(spec_id.into());
            }
            if plan.primary_spec_id.is_none() {
                plan.primary_spec_id = Some(spec_id.into());
            }
            let link = SpecLink {
                spec_id: spec_id.into(),
                spec_section_id: spec_section_id.into(),
                plan_section_id: plan_section_id.into(),
            };
            if !plan.spec_links.iter().any(|existing| existing == &link) {
                plan.spec_links.push(link);
            }
            Ok(())
        })?;
        // First link claims ownership: co-locate the spec into this plan.
        if spec.plan_id.as_deref() != Some(plan_id) {
            self.adopt_spec_to_plan(spec_id, plan_id, now_ms)?;
        }
        Ok(())
    }

    pub fn unlink_spec_section(
        &self,
        plan_id: &str,
        spec_id: &str,
        spec_section_id: Option<&str>,
        plan_section_id: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            plan.spec_links.retain(|link| {
                !(link.spec_id == spec_id
                    && spec_section_id.is_none_or(|sid| link.spec_section_id == sid)
                    && plan_section_id.is_none_or(|sid| link.plan_section_id == sid))
            });
            if !plan.spec_links.iter().any(|link| link.spec_id == spec_id) {
                plan.spec_refs.retain(|id| id != spec_id);
                if plan.primary_spec_id.as_deref() == Some(spec_id) {
                    plan.primary_spec_id = plan.spec_refs.first().cloned();
                }
            }
            Ok(())
        })
    }

    pub fn delete_spec(&self, spec_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let spec = self.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        // Scrub references from every plan that points at this spec.
        let referencing: Vec<String> = self
            .plans
            .read()
            .iter()
            .filter(|(_, plan)| plan.lock().spec_refs.iter().any(|r| r == spec_id))
            .map(|(id, _)| id.clone())
            .collect();
        for plan_id in referencing {
            self.with_plan(&plan_id, now_ms, |plan| {
                plan.spec_refs.retain(|r| r != spec_id);
                plan.spec_links.retain(|l| l.spec_id != spec_id);
                if plan.primary_spec_id.as_deref() == Some(spec_id) {
                    plan.primary_spec_id = None;
                }
                Ok(())
            })?;
        }
        self.database.delete_spec(&spec.id, now_ms)?;
        #[cfg(test)]
        self.remove_test_spec_sidecars(&spec)?;
        self.specs.write().remove(spec_id);
        self.spec_index.write().remove(spec_id);
        Ok(())
    }

    #[cfg(test)]
    fn remove_test_spec_sidecars(&self, spec: &Spec) -> Result<(), PlanError> {
        let Some(root) = super::paths::test_project_root(super::paths::spec_sidecar_root(spec))
        else {
            return Ok(());
        };
        let specs_dir =
            super::paths::specs_dir(&root).map_err(|error| PlanError::Io(error.to_string()))?;
        let global = super::spec_store::spec_yaml_path(&specs_dir, &spec.id);
        let _ = std::fs::remove_file(global);
        if let Some(plan_id) = spec.plan_id.as_deref() {
            if let Some(plan) = self.get(plan_id) {
                let mut normalized = plan.clone();
                super::paths::normalize_plan_paths(&mut normalized, &root);
                let dir = super::store_v2::plan_state_dir(&normalized)
                    .map_err(|error| PlanError::Io(error.to_string()))?;
                let _ = std::fs::remove_file(super::spec_store::spec_yaml_path_in_plan(&dir));
            }
        }
        Ok(())
    }

    pub fn list_spec_links(&self, spec_id: &str) -> Vec<(String, SpecLink)> {
        self.plans
            .read()
            .values()
            .flat_map(|plan| {
                let plan = plan.lock();
                plan.spec_links
                    .iter()
                    .filter(|link| link.spec_id == spec_id)
                    .cloned()
                    .map(|link| (plan.id.clone(), link))
                    .collect::<Vec<_>>()
            })
            .collect()
    }

    pub fn link_effort(
        &self,
        from: &str,
        to: &str,
        rel: EffortLinkRel,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(from, now_ms, |plan| {
            let link = EffortLink { to: to.into(), rel };
            if !plan.lineage.iter().any(|existing| existing == &link) {
                plan.lineage.push(link);
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};

    fn store_root() -> (std::path::PathBuf, PlanStore) {
        let root = tempfile::tempdir().unwrap().keep();
        (root.clone(), PlanStore::new_for_test(&root))
    }

    fn create_plan(store: &PlanStore, root: &std::path::Path, title: &str) -> Plan {
        store
            .create_plan(
                title.into(),
                root.display().to_string(),
                OrchestratorRef::default(),
                vec![],
                1,
            )
            .unwrap()
    }

    #[test]
    fn orphan_spec_persists_inline_yaml_and_reloads() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Requirements".into(), root.display().to_string(), 10)
            .unwrap();
        let sid = store
            .add_spec_section(&spec.id, "Login", "body text", None, &[], None, None, 11)
            .unwrap();

        // Orphan (no owning plan) -> global yaml, bodies inline, no json/md fan-out.
        let yaml_path = root
            .join(".codeterm")
            .join("specs")
            .join(format!("{}.yaml", spec.id));
        assert!(yaml_path.is_file());
        assert!(!root.join(".codeterm/specs").join(&spec.id).exists());
        assert!(std::fs::read_to_string(&yaml_path)
            .unwrap()
            .contains("body text"));

        let reloaded = PlanStore::new_for_test(&root);
        reloaded.scan_project(&root).unwrap();
        let spec = reloaded.get_spec(&spec.id).unwrap();
        assert_eq!(spec.sections[0].id, sid);
        assert_eq!(spec.sections[0].body, "body text");
        assert_eq!(spec.plan_id, None);
    }

    #[test]
    fn linking_adopts_spec_into_plan_dir() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Reqs".into(), root.display().to_string(), 1)
            .unwrap();
        let ssec = store
            .add_spec_section(&spec.id, "Req", "", None, &[], None, None, 2)
            .unwrap();
        let plan = create_plan(&store, &root, "Plan");
        let psec = store
            .add_section(&plan.id, "Build", "", None, &[], None, None, 3)
            .unwrap();
        let global = root
            .join(".codeterm/specs")
            .join(format!("{}.yaml", spec.id));
        assert!(global.is_file());

        store
            .link_spec_section(&plan.id, &spec.id, &ssec, &psec, 4)
            .unwrap();

        let co_located =
            crate::service_plan::store_v2::plan_state_dir(&store.get(&plan.id).unwrap())
                .unwrap()
                .join("spec.yaml");
        assert!(co_located.is_file(), "spec.yaml must land in the plan dir");
        assert!(
            !global.exists(),
            "orphan global yaml must be dropped on adoption"
        );
        assert_eq!(
            store.get_spec(&spec.id).unwrap().plan_id.as_deref(),
            Some(plan.id.as_str())
        );
    }

    #[test]
    fn spec_coverage_reports_unplanned_and_two_hop_done() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Requirements".into(), root.display().to_string(), 10)
            .unwrap();
        let linked = store
            .add_spec_section(&spec.id, "Linked", "", None, &[], None, None, 11)
            .unwrap();
        store
            .add_spec_section(&spec.id, "Unplanned", "", None, &[], None, None, 12)
            .unwrap();
        let plan = create_plan(&store, &root, "Plan");
        let plan_sec = store
            .add_section(&plan.id, "Build", "", None, &[], None, None, 13)
            .unwrap();
        let task = store
            .add_task(
                &plan.id,
                crate::service_plan::NewTask {
                    name: "Do it".into(),
                    kind: "implement".into(),
                    section_ids: vec![plan_sec.clone()],
                    competition: None,
                    ..Default::default()
                },
                14,
            )
            .unwrap();
        store
            .update_task(
                &plan.id,
                &task,
                None,
                None,
                None,
                None,
                Some("done"),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
                None,
                None,
                15,
            )
            .unwrap();
        store
            .link_spec_section(&plan.id, &spec.id, &linked, &plan_sec, 16)
            .unwrap();

        let report = store.spec_coverage_report(&spec.id).unwrap();
        assert_eq!(report.unplanned, vec!["Unplanned".to_string()]);
        assert_eq!(report.sections[0].status, "done");
        assert_eq!(report.sections[1].status, "unplanned");
    }

    #[test]
    fn batch_spec_ops_resolve_refs_and_link_grid() {
        let (root, store) = store_root();
        let plan = create_plan(&store, &root, "Plan");
        let plan_sec = store
            .add_section(&plan.id, "Build", "", None, &[], None, None, 2)
            .unwrap();
        let result = store
            .apply_batch(
                &plan.id,
                crate::service_plan::batch::BatchRequest {
                    key: None,
                    ops: vec![
                        crate::service_plan::batch::BatchOp::UpsertSpec {
                            client_id: Some("sp".into()),
                            spec_id: None,
                            title: "Spec".into(),
                        },
                        crate::service_plan::batch::BatchOp::UpsertSpecSection {
                            client_id: Some("ss".into()),
                            spec: "$sp".into(),
                            section_id: None,
                            title: "Req".into(),
                            body: Some("body".into()),
                        },
                        crate::service_plan::batch::BatchOp::LinkSpecSection {
                            spec: "$sp".into(),
                            spec_section: "$ss".into(),
                            plan_section: plan_sec.clone(),
                        },
                    ],
                },
                3,
            )
            .unwrap();
        let spec_id = result.ids.get("sp").unwrap();
        let plan = store.get(&plan.id).unwrap();
        assert_eq!(plan.spec_refs, vec![spec_id.clone()]);
        assert_eq!(plan.spec_links.len(), 1);
    }

    #[test]
    fn archive_carries_co_located_spec_yaml() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Reqs".into(), root.display().to_string(), 1)
            .unwrap();
        store
            .add_spec_section(&spec.id, "Req", "req body", None, &[], None, None, 2)
            .unwrap();
        let plan = store
            .create_plan_from_spec(
                &spec.id,
                "Plan".into(),
                OrchestratorRef::default(),
                vec![],
                3,
            )
            .unwrap();
        let active_spec = root
            .join(".codeterm/plans/active")
            .join(&plan.id)
            .join("spec.yaml");
        assert!(active_spec.is_file());

        store.archive(&plan.id, 4).unwrap();

        let archived_spec = root
            .join(".codeterm/plans/archive")
            .join(&plan.id)
            .join("spec.yaml");
        assert!(
            archived_spec.is_file(),
            "spec.yaml must move with its plan on archive"
        );
        assert!(!active_spec.exists());
        let text = std::fs::read_to_string(&archived_spec).unwrap();
        assert!(text.contains("req body"));
    }

    #[test]
    fn plan_delete_evicts_owned_spec_from_memory() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Reqs".into(), root.display().to_string(), 1)
            .unwrap();
        store
            .add_spec_section(&spec.id, "Req", "body", None, &[], None, None, 2)
            .unwrap();
        let plan = store
            .create_plan_from_spec(
                &spec.id,
                "Plan".into(),
                OrchestratorRef::default(),
                vec![],
                3,
            )
            .unwrap();
        assert_eq!(
            store.get_spec(&spec.id).unwrap().plan_id.as_deref(),
            Some(plan.id.as_str())
        );

        store.delete(&plan.id).unwrap();

        assert!(
            store.get_spec(&spec.id).is_none(),
            "owned spec must die with its plan"
        );
        assert!(!store.list_spec_index().iter().any(|e| e.spec_id == spec.id));
    }

    #[test]
    fn delete_spec_removes_orphan_file_and_memory() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Loose".into(), root.display().to_string(), 1)
            .unwrap();
        let yaml = root
            .join(".codeterm/specs")
            .join(format!("{}.yaml", spec.id));
        assert!(yaml.is_file());

        store.delete_spec(&spec.id, 2).unwrap();

        assert!(!yaml.exists());
        assert!(store.get_spec(&spec.id).is_none());
    }

    #[test]
    fn delete_spec_scrubs_plan_references() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Reqs".into(), root.display().to_string(), 1)
            .unwrap();
        let ssec = store
            .add_spec_section(&spec.id, "Req", "", None, &[], None, None, 2)
            .unwrap();
        let plan = create_plan(&store, &root, "Plan");
        let psec = store
            .add_section(&plan.id, "Build", "", None, &[], None, None, 3)
            .unwrap();
        store
            .link_spec_section(&plan.id, &spec.id, &ssec, &psec, 4)
            .unwrap();

        store.delete_spec(&spec.id, 5).unwrap();

        let plan = store.get(&plan.id).unwrap();
        assert!(plan.spec_refs.is_empty());
        assert!(plan.spec_links.is_empty());
        assert_eq!(plan.primary_spec_id, None);
        let co_located = crate::service_plan::store_v2::plan_state_dir(&plan)
            .unwrap()
            .join("spec.yaml");
        assert!(!co_located.exists(), "co-located yaml must be removed");
    }

    #[test]
    fn from_spec_conflict_leaves_no_orphan_plan() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("SCRATCH-dx-spec".into(), root.display().to_string(), 1)
            .unwrap();
        let owner = store
            .create_plan_from_spec(
                &spec.id,
                "SCRATCH-owner-plan".into(),
                OrchestratorRef::default(),
                vec![],
                2,
            )
            .unwrap();
        assert_eq!(store.list_full().len(), 1);

        let conflict = store.create_plan_from_spec(
            &spec.id,
            "SCRATCH-conflict-plan".into(),
            OrchestratorRef::default(),
            vec![],
            3,
        );
        assert!(conflict.is_err(), "second from-spec must be rejected");
        assert_eq!(
            store.list_full().len(),
            1,
            "spec-conflict must leave zero orphan plans"
        );
        assert_eq!(
            store.get_spec(&spec.id).unwrap().plan_id.as_deref(),
            Some(owner.id.as_str())
        );
    }

    #[test]
    fn effort_lineage_survives_archive() {
        let (root, store) = store_root();
        let a = create_plan(&store, &root, "A");
        let b = create_plan(&store, &root, "B");
        store
            .link_effort(&a.id, &b.id, EffortLinkRel::Follows, 2)
            .unwrap();
        store.archive(&a.id, 3).unwrap();
        let archived = store.get(&a.id).unwrap();
        assert_eq!(archived.status, crate::plan::PlanStatus::Archived);
        assert_eq!(archived.lineage[0].to, b.id);
    }
}
