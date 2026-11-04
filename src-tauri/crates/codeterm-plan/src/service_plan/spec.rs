use super::mutations::{
    push_subsection, rm_section_tree, update_section_fields, update_section_tags,
};
use super::{MutationWriterTurn, PlanError, PlanStore};
use crate::plan::{EffortLink, EffortLinkRel, PlanStatus, Section, Spec, SpecLink};

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
        super::text_bounds::validate_plan_label("plan title", &title)?;
        let team_id = team_id.trim().to_string();
        if team_id.is_empty() {
            return Err(PlanError::ActivePlanRequiresTeam {
                plan_id: "new plan".into(),
            });
        }
        let turn = self.mutation_writer.enter();
        let (plan, receipt) = self.database.create_spec_plan_rows(
            super::store_db::hot_write::SpecPlanCreate {
                spec_id: spec_id.into(),
                title,
                owner,
                team_id,
                kinds,
            },
            now_ms,
        )?;
        self.publish_hot_receipt(&turn, receipt);
        super::search_index::enqueue_plan_import(&turn, &plan);
        self.publish_product_created();
        Ok(plan)
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
        let (out, receipt) = self.database.edit_spec_rows(id, now_ms, f)?;
        self.publish_hot_receipt(turn, receipt);
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
        let turn = self.mutation_writer.enter();
        let receipt = self.database.change_spec_link(
            plan_id,
            super::store_db::hot_write::SpecLinkChange::Link(SpecLink {
                spec_id: spec_id.into(),
                spec_section_id: spec_section_id.into(),
                plan_section_id: plan_section_id.into(),
            }),
            now_ms,
        )?;
        self.publish_hot_receipt(&turn, receipt);
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
        let turn = self.mutation_writer.enter();
        let receipt = self.database.change_spec_link(
            plan_id,
            super::store_db::hot_write::SpecLinkChange::Unlink {
                spec_id: spec_id.into(),
                spec_section_id: spec_section_id.map(str::to_string),
                plan_section_id: plan_section_id.map(str::to_string),
            },
            now_ms,
        )?;
        self.publish_hot_receipt(&turn, receipt);
        Ok(())
    }

    pub fn delete_spec(&self, spec_id: &str, now_ms: u64) -> Result<(), PlanError> {
        let turn = self.mutation_writer.enter();
        let (spec, receipts) = self.database.delete_spec_rows(spec_id, now_ms)?;
        #[cfg(test)]
        self.database.remove_spec_test_sidecars(&spec)?;
        #[cfg(test)]
        let work = receipts
            .iter()
            .map(super::store_db::hot_write::work_values)
            .fold([0; 14], |mut sum, values| {
                for (total, value) in sum.iter_mut().zip(values) {
                    *total += value;
                }
                sum
            });
        for receipt in receipts {
            self.publish_hot_receipt(&turn, receipt);
        }
        #[cfg(test)]
        super::store_db::hot_write::record_work_values(work);
        self.specs.write().remove(spec_id);
        self.spec_index.write().remove(spec_id);
        super::search_index::remove_spec_from_index(&turn, spec);
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
        self.commit_header_change(
            from,
            super::store_db::hot_write::HeaderChange::Effort(EffortLink { to: to.into(), rel }),
            now_ms,
        )
        .map(|_| ())
    }
}

#[cfg(test)]
mod tests;
