use super::*;

#[derive(Clone, PartialEq, Eq)]
struct Identity {
    title: Option<String>,
    session: Option<String>,
    nonce: Option<String>,
    launch_profile: Option<String>,
}
impl Identity {
    fn read(attempt: &TaskExecutionAttempt) -> Self {
        Self {
            title: attempt.executor_title.clone(),
            session: attempt.session_id.clone(),
            nonce: attempt.session_binding_nonce.clone(),
            launch_profile: attempt.launch_profile.clone(),
        }
    }
    fn apply(self, attempt: &mut TaskExecutionAttempt) {
        attempt.executor_title = self.title;
        attempt.session_id = self.session;
        attempt.session_binding_nonce = self.nonce;
        attempt.launch_profile = self.launch_profile;
    }
}

pub struct AttemptExecutorIdentityChange {
    plan_id: String,
    task_id: String,
    attempt_id: String,
    tab_id: String,
    before: Identity,
    after: Identity,
}

impl PlanStore {
    #[allow(clippy::too_many_arguments)]
    pub fn update_attempt_executor_identity(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        title: Option<&str>,
        session_id: Option<&str>,
        nonce: Option<&str>,
        launch_profile: Option<&str>,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        self.update_attempt_executor_identity_with_undo(
            plan_id,
            task_id,
            tab_id,
            title,
            session_id,
            nonce,
            launch_profile,
            now_ms,
        )
        .map(|_| ())
    }

    #[allow(clippy::too_many_arguments)]
    pub fn update_attempt_executor_identity_with_undo(
        &self,
        plan_id: &str,
        task_id: &str,
        tab_id: &str,
        title: Option<&str>,
        session_id: Option<&str>,
        nonce: Option<&str>,
        launch_profile: Option<&str>,
        now_ms: u64,
    ) -> Result<AttemptExecutorIdentityChange, PlanError> {
        let normalize = |value: Option<&str>| {
            value
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_owned)
        };
        let after = Identity {
            title: normalize(title),
            session: normalize(session_id),
            nonce: normalize(nonce),
            launch_profile: launch_profile.map(str::to_owned),
        };
        self.change_attempt(
            plan_id,
            task_id,
            AttemptTarget {
                tab: tab_id,
                ..Default::default()
            },
            now_ms,
            |attempt| {
                let attempt = attempt.ok_or(PlanError::TaskNotFound)?;
                let before = Identity::read(attempt);
                let after = Identity {
                    launch_profile: after
                        .launch_profile
                        .clone()
                        .or_else(|| before.launch_profile.clone()),
                    ..after.clone()
                };
                let receipt = AttemptExecutorIdentityChange {
                    plan_id: plan_id.into(),
                    task_id: task_id.into(),
                    attempt_id: attempt.id.clone(),
                    tab_id: tab_id.into(),
                    before,
                    after: after.clone(),
                };
                after.apply(attempt);
                Ok(Some(receipt))
            },
        )?
        .ok_or(PlanError::TaskNotFound)
    }

    pub fn undo_attempt_executor_identity(
        &self,
        receipt: AttemptExecutorIdentityChange,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if receipt.before == receipt.after {
            return Ok(());
        }
        self.change_attempt(
            &receipt.plan_id,
            &receipt.task_id,
            AttemptTarget {
                tab: &receipt.tab_id,
                id: Some(&receipt.attempt_id),
                ..Default::default()
            },
            now_ms,
            |attempt| {
                let attempt = attempt.ok_or(PlanError::TaskNotFound)?;
                if Identity::read(attempt) != receipt.after {
                    return Err(PlanError::Io(
                        "executor identity rollback conflict: a newer identity was persisted"
                            .into(),
                    ));
                }
                receipt.before.apply(attempt);
                Ok(Some(()))
            },
        )
        .map(|_| ())
    }
}
