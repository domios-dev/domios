use super::mutations::{
    has_section, push_subsection, rm_section_tree, update_section_fields, update_section_tags,
};
use super::{MutationWriterTurn, PlanError, PlanStore};
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
        let team_id = self.mint_id("team_test")?;
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
        let spec = self.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        // Claim ownership before inserting the plan row: a spec-conflict must
        // leave zero orphan plans behind.
        if let Some(owner) = spec.plan_id.as_deref() {
            return Err(PlanError::Io(format!(
                "spec {spec_id} already owned by plan {owner}"
            )));
        }
        let mut plan = self.create_plan_for_team_unobserved(
            title,
            spec.project_path.clone(),
            owner,
            team_id,
            kinds,
            now_ms,
        )?;
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
        self.publish_product_created();
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
        if self.id_taken(&spec.id)? {
            spec.id = self.mint_id("sp")?;
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
        let turn = self.mutation_writer.enter();
        self.with_spec_in_writer(&turn, id, now_ms, f)
    }

    fn with_spec_in_writer<T, F>(
        &self,
        turn: &MutationWriterTurn<'_>,
        id: &str,
        now_ms: u64,
        f: F,
    ) -> Result<T, PlanError>
    where
        F: FnOnce(&mut Spec) -> Result<T, PlanError>,
    {
        turn.assert_store(self);
        let arc = self
            .specs
            .read()
            .get(id)
            .cloned()
            .ok_or(PlanError::NotFound)?;
        let mut staged = arc.lock().clone();
        let out = f(&mut staged)?;
        staged.updated_at = now_ms;
        if staged.canonical_project_path.trim().is_empty() {
            staged.canonical_project_path = staged.project_path.clone();
        }
        super::tag_model::normalize_sections_for_spec(&mut staged.sections);
        let path = self
            .persist_spec_in_writer(turn, &staged)
            .map_err(|e| PlanError::Io(e.to_string()))?;
        let entry = Self::spec_index_entry(&staged, path);
        self.spec_index.write().insert(staged.id.clone(), entry);
        *arc.lock() = staged;
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
        let sid = self.mint_id("sec")?;
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
        let turn = self.mutation_writer.enter();
        let spec = self.get_spec(spec_id).ok_or(PlanError::NotFound)?;
        // Scrub references from every plan that points at this spec.
        let referencing = self
            .database
            .plan_ids_matching(&super::PlanMatch::SpecRef(spec_id.to_string()))?;
        for plan_id in referencing {
            self.with_plan_in_writer(&turn, &plan_id, now_ms, |plan| {
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
        super::search_index::remove_spec_from_index(&turn, spec);
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

    pub fn list_spec_links(&self, spec_id: &str) -> Result<Vec<(String, SpecLink)>, PlanError> {
        self.database.spec_links(spec_id)
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
mod tests;
