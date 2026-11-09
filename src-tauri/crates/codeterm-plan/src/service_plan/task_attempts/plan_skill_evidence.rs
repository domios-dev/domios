use super::super::{PlanError, PlanStore};

impl PlanStore {
    pub fn record_plan_skill_load(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        session_id: &str,
        session_binding_nonce: &str,
        attempt_id: Option<&str>,
        slug: &str,
        content_digest: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Binding {
                attempt: attempt_id.unwrap_or_default().into(),
                tab: tab_id.into(),
            },
            now_ms,
            |edit| {
                let current = super::super::task_authority_projection::current_skill_from_rows(
                    &edit.tab_owners,
                    tab_id,
                    session_id,
                    session_binding_nonce,
                )?
                .ok_or_else(|| {
                    PlanError::Io("plan skill caller has no current assignment".into())
                })?;
                if current.0 != plan_id
                    || current.1 != task_id
                    || attempt_id.is_some_and(|id| id != current.2)
                {
                    return Err(PlanError::Io(
                        "plan skill target conflicts with current assignment".into(),
                    ));
                }
                let task = &mut edit.task;
                if task.id != task_id {
                    return Err(PlanError::TaskNotFound);
                }
                let matching_attempts = task
                    .execution_attempts
                    .iter()
                    .enumerate()
                    .filter_map(|(index, attempt)| {
                        (attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                            && attempt.executor_tab_id == tab_id
                            && attempt.session_id.as_deref() == Some(session_id)
                            && attempt.session_binding_nonce.as_deref()
                                == Some(session_binding_nonce)
                            && attempt_id.is_none_or(|attempt_id| attempt.id == attempt_id))
                        .then_some(index)
                    })
                    .collect::<Vec<_>>();
                let [attempt_index] = matching_attempts.as_slice() else {
                    return Err(PlanError::Io(
                        "plan skill caller does not identify exactly one current attempt".into(),
                    ));
                };
                let attempt = &mut task.execution_attempts[*attempt_index];
                let announced = attempt
                    .plan_skill_manifest
                    .iter()
                    .find(|entry| entry.slug == slug && entry.content_digest == content_digest)
                    .ok_or_else(|| {
                        PlanError::Io("plan skill was not announced with this digest".into())
                    })?;
                if attempt.loaded_plan_skills.iter().any(|evidence| {
                    evidence.slug == announced.slug
                        && evidence.content_digest == announced.content_digest
                }) {
                    return Ok(());
                }
                attempt
                    .loaded_plan_skills
                    .push(crate::plan::LoadedPlanSkillEvidence {
                        slug: announced.slug.clone(),
                        content_digest: announced.content_digest.clone(),
                        loaded_at: now_ms,
                    });
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }

    pub fn current_plan_skill_attempt(
        &self,
        tab_id: &str,
        session_id: &str,
        nonce: &str,
    ) -> Result<Option<(String, String, String)>, PlanError> {
        if tab_id.trim().is_empty() || session_id.trim().is_empty() || nonce.trim().is_empty() {
            return Err(PlanError::Io(
                "plan skill caller identity is incomplete".into(),
            ));
        }
        self.database
            .current_skill_attempt(tab_id, session_id, nonce)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn announce_current_plan_skill_manifest(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        tab_id: &str,
        session_id: &str,
        nonce: &str,
        now_ms: u64,
    ) -> Result<Vec<crate::plan::PlanSkillManifestEntry>, PlanError> {
        let mut announced = Vec::new();
        let turn = self.mutation_writer.enter();
        let manifest = super::super::plan_skills::plan_skill_manifest(self, plan_id)?;
        self.with_task_rows_in_writer(
            &turn,
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Binding {
                attempt: attempt_id.into(),
                tab: tab_id.into(),
            },
            now_ms,
            |edit| {
                let current = super::super::task_authority_projection::current_skill_from_rows(
                    &edit.tab_owners,
                    tab_id,
                    session_id,
                    nonce,
                )?;
                if current
                    .as_ref()
                    .map(|(plan, task, attempt)| (plan.as_str(), task.as_str(), attempt.as_str()))
                    != Some((plan_id, task_id, attempt_id))
                {
                    return Err(PlanError::Io(
                        "plan skill announcement target is not current".into(),
                    ));
                }
                let superseded = edit.superseded;
                let task = &mut edit.task;
                let open = task
                    .execution_attempts
                    .iter()
                    .filter(|attempt| {
                        attempt.executor_tab_id == tab_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
                    .collect::<Vec<_>>();
                let [attempt] = open.as_slice() else {
                    return Err(PlanError::Io("plan skill announcement is ambiguous".into()));
                };
                if attempt.id != attempt_id
                    || attempt.session_id.as_deref() != Some(session_id)
                    || attempt.session_binding_nonce.as_deref() != Some(nonce)
                    || attempt.interruption.owner_epoch != task.interruption.owner_epoch
                {
                    return Err(PlanError::Io(
                        "plan skill announcement binding is not current".into(),
                    ));
                }
                if attempt.acknowledged_at.is_none() && superseded {
                    return Err(PlanError::Io(
                        "plan skill announcement requires acknowledgement for a reassigned attempt"
                            .into(),
                    ));
                }
                // Only content identity matters; a reworded purpose must not rewrite the attempt.
                let same_content =
                    attempt.plan_skill_manifest.len() == manifest.len()
                        && attempt.plan_skill_manifest.iter().zip(&manifest).all(
                            |(held, current)| {
                                held.slug == current.slug
                                    && held.content_digest == current.content_digest
                            },
                        );
                if same_content {
                    announced = attempt.plan_skill_manifest.clone();
                    return Ok(None);
                }
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id)
                    .ok_or(PlanError::TaskNotFound)?;
                attempt.plan_skill_manifest = manifest.clone();
                announced = manifest;
                task.updated_at = now_ms;
                Ok(Some(()))
            },
        )?;
        Ok(announced)
    }

    pub fn set_attempt_plan_skill_manifest(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        manifest: Vec<crate::plan::PlanSkillManifestEntry>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_task_rows(
            plan_id,
            task_id,
            super::super::store_db::hot_write::TaskHistory::Attempt(attempt_id.into()),
            now_ms,
            |edit| {
                let task = &mut edit.task;
                let attempt = task
                    .execution_attempts
                    .iter_mut()
                    .find(|attempt| attempt.id == attempt_id && attempt.closed_at.is_none())
                    .ok_or(PlanError::TaskNotFound)?;
                attempt.plan_skill_manifest = manifest;
                task.updated_at = now_ms;
                Ok(())
            },
        )
    }
}
