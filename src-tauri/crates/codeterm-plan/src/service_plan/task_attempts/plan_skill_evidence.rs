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

    pub fn set_attempt_plan_skill_manifest(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        manifest: Vec<crate::plan::PlanSkillManifestEntry>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.with_plan(plan_id, now_ms, |plan| {
            let task = plan
                .tasks
                .iter_mut()
                .find(|task| task.id == task_id || task.handle.as_deref() == Some(task_id))
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
