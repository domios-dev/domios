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
        self.with_plan(plan_id, now_ms, |plan| {
            let current = self
                .current_plan_skill_attempt(tab_id, session_id, session_binding_nonce)?
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
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let matching_attempts = task
                .execution_attempts
                .iter()
                .enumerate()
                .filter_map(|(index, attempt)| {
                    (attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                        && attempt.executor_tab_id == tab_id
                        && attempt.session_id.as_deref() == Some(session_id)
                        && attempt.session_binding_nonce.as_deref() == Some(session_binding_nonce)
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
        })
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
        let mut current = None;
        for plan in self
            .list_full()
            .into_iter()
            .filter(|plan| plan.status == crate::plan::PlanStatus::Active)
        {
            for task in plan.tasks.iter().filter(|task| !task.status.is_terminal()) {
                for attempt in task.execution_attempts.iter().filter(|attempt| {
                    attempt.executor_tab_id == tab_id
                        && attempt.closed_at.is_none()
                        && attempt.retired_at.is_none()
                }) {
                    if current.is_some()
                        || (attempt.competition_group_id.is_none()
                            && task.assigned_tab_id.as_deref() != Some(tab_id))
                        || attempt.session_id.as_deref() != Some(session_id)
                        || attempt.session_binding_nonce.as_deref() != Some(nonce)
                    {
                        return Err(PlanError::Io(
                            "plan skill caller does not identify exactly one current attempt"
                                .into(),
                        ));
                    }
                    current = Some((plan.id.clone(), task.id.clone(), attempt.id.clone()));
                }
                if task.assigned_tab_id.as_deref() == Some(tab_id)
                    && !task.execution_attempts.iter().any(|attempt| {
                        attempt.executor_tab_id == tab_id
                            && attempt.closed_at.is_none()
                            && attempt.retired_at.is_none()
                    })
                {
                    return Err(PlanError::Io(
                        "plan skill assignment has no current attempt".into(),
                    ));
                }
            }
        }
        Ok(current)
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
        self.with_plan_if_changed(plan_id, now_ms, |plan| {
            let current = self.current_plan_skill_attempt(tab_id, session_id, nonce)?;
            if current
                .as_ref()
                .map(|(plan, task, attempt)| (plan.as_str(), task.as_str(), attempt.as_str()))
                != Some((plan_id, task_id, attempt_id))
            {
                return Err(PlanError::Io(
                    "plan skill announcement target is not current".into(),
                ));
            }
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
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
                || attempt.acknowledged_at.is_none()
            {
                return Err(PlanError::Io(
                    "plan skill announcement binding is not current".into(),
                ));
            }
            announced = attempt.plan_skill_manifest.clone();
            if !announced.is_empty() {
                return Ok(None);
            }
            let manifest = super::super::plan_skills::plan_skill_manifest(self, plan_id)?;
            if manifest.is_empty() {
                return Ok(None);
            }
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id)
                .ok_or(PlanError::TaskNotFound)?;
            attempt.plan_skill_manifest = manifest.clone();
            announced = manifest.clone();
            task.updated_at = now_ms;
            Ok(Some(()))
        })?;
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
        self.with_plan(plan_id, now_ms, |plan| {
            let task = crate::plan::task_key::find_task_mut(&mut plan.tasks, task_id)
                .ok_or(PlanError::TaskNotFound)?;
            let attempt = task
                .execution_attempts
                .iter_mut()
                .find(|attempt| attempt.id == attempt_id && attempt.closed_at.is_none())
                .ok_or(PlanError::TaskNotFound)?;
            attempt.plan_skill_manifest = manifest;
            task.updated_at = now_ms;
            Ok(())
        })
    }
}
