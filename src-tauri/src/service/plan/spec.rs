use super::mutations::{
    has_section, push_subsection, rm_section_tree, update_section_fields, update_section_tags,
};
use super::{PlanError, PlanStore};
use crate::plan::{EffortLink, EffortLinkRel, PlanStatus, Section, Spec, SpecLink};

fn gen_id(prefix: &str) -> String {
    crate::plan::ids::short_id(prefix)
}

impl PlanStore {
    pub fn create_plan_from_spec(
        &self,
        spec_id: &str,
        title: String,
        owner: crate::plan::OrchestratorRef,
        kinds: Vec<crate::plan::MilestoneKind>,
        now_ms: u64,
    ) -> Result<crate::plan::Plan, PlanError> {
        let spec = self.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        let mut plan = self.create_plan(title, spec.project_path.clone(), owner, kinds, now_ms)?;
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
        Ok(plan)
    }

    pub fn create_spec(
        &self,
        title: String,
        project_path: String,
        now_ms: u64,
    ) -> Result<Spec, PlanError> {
        let project_path =
            crate::service::plan::paths::project_root(std::path::Path::new(&project_path))
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
        let path = Self::write_spec_atomic(&guard).map_err(|e| PlanError::Io(e.to_string()))?;
        self.note_spec_write(&guard, &path);
        self.mirror_spec_index(&guard)
            .map_err(|e| PlanError::Io(e.to_string()))?;
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
        })
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
    fn spec_storage_roundtrip_externalizes_sections() {
        let (root, store) = store_root();
        let spec = store
            .create_spec("Requirements".into(), root.display().to_string(), 10)
            .unwrap();
        let sid = store
            .add_spec_section(&spec.id, "Login", "body text", None, &[], None, None, 11)
            .unwrap();

        let json_path = root
            .join(".codeterm")
            .join("specs")
            .join(format!("{}.json", spec.id));
        let body_path = root
            .join(".codeterm")
            .join("specs")
            .join(&spec.id)
            .join("sections")
            .join("login.md");
        assert!(json_path.exists());
        assert_eq!(std::fs::read_to_string(&body_path).unwrap(), "body text");
        assert!(!std::fs::read_to_string(&json_path)
            .unwrap()
            .contains("body text"));

        let reloaded = PlanStore::new_for_test(&root);
        reloaded.scan_project(&root).unwrap();
        let spec = reloaded.get_spec(&spec.id).unwrap();
        assert_eq!(spec.sections[0].id, sid);
        assert_eq!(spec.sections[0].body, "body text");
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
                crate::service::plan::NewTask {
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
                crate::service::plan::batch::BatchRequest {
                    key: None,
                    ops: vec![
                        crate::service::plan::batch::BatchOp::UpsertSpec {
                            client_id: Some("sp".into()),
                            spec_id: None,
                            title: "Spec".into(),
                        },
                        crate::service::plan::batch::BatchOp::UpsertSpecSection {
                            client_id: Some("ss".into()),
                            spec: "$sp".into(),
                            section_id: None,
                            title: "Req".into(),
                            body: Some("body".into()),
                        },
                        crate::service::plan::batch::BatchOp::LinkSpecSection {
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
