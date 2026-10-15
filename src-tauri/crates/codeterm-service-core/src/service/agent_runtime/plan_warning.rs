use crate::{multiplexer::types::TaskBinding, service::ServiceContext};
use serde::Serialize;

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanBindingWarningStage {
    Finalization,
    RestartAuthorization,
    RestartCallerRestoration,
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PlanBindingFailureKind {
    AuthorityViolation,
    BookkeepingUnavailable,
}

#[derive(Debug)]
pub(crate) struct PlanBindingFailure {
    pub kind: PlanBindingFailureKind,
    pub reason: String,
}

impl PlanBindingFailure {
    pub(crate) fn bookkeeping(reason: impl ToString) -> Self {
        Self {
            kind: PlanBindingFailureKind::BookkeepingUnavailable,
            reason: reason.to_string(),
        }
    }

    pub(crate) fn from_plan_error(error: crate::service::plan::PlanError) -> Self {
        if matches!(
            error,
            crate::service::plan::PlanError::AttemptBindingRestoreRefused { .. }
        ) {
            Self {
                kind: PlanBindingFailureKind::AuthorityViolation,
                reason: error.to_string(),
            }
        } else {
            Self::bookkeeping(error)
        }
    }
}

impl From<&str> for PlanBindingFailure {
    fn from(reason: &str) -> Self {
        Self {
            kind: PlanBindingFailureKind::AuthorityViolation,
            reason: reason.into(),
        }
    }
}

impl From<String> for PlanBindingFailure {
    fn from(reason: String) -> Self {
        Self {
            kind: PlanBindingFailureKind::AuthorityViolation,
            reason,
        }
    }
}

#[derive(Debug, Serialize)]
struct PlanBindingWarning<'a> {
    kind: &'static str,
    stage: PlanBindingWarningStage,
    tab_id: &'a str,
    plan_id: Option<&'a str>,
    task_id: Option<&'a str>,
    attempt_id: Option<&'a str>,
    failure_kind: PlanBindingFailureKind,
    reason: &'a str,
}

impl ServiceContext {
    /// Returns the notice text when the bound plan could not record the warning as a stride.
    pub(crate) fn warn_runtime_plan_binding(
        &self,
        tab_id: &str,
        stage: PlanBindingWarningStage,
        failure: &PlanBindingFailure,
    ) -> Option<String> {
        let binding = {
            let mut mux = self.mux.write();
            mux.get_tab_mut(tab_id).and_then(|tab| {
                if matches!(failure.kind, PlanBindingFailureKind::AuthorityViolation) {
                    tab.task_binding.take()
                } else {
                    tab.task_binding.clone()
                }
            })
        };
        self.retain_runtime_restart_binding(tab_id, None, "");
        let warning = PlanBindingWarning {
            kind: "runtime_plan_binding_warning",
            stage,
            tab_id,
            plan_id: binding
                .as_ref()
                .and_then(|binding| binding.plan_id.as_deref()),
            task_id: binding
                .as_ref()
                .and_then(|binding| binding.task_id.as_deref()),
            attempt_id: binding
                .as_ref()
                .and_then(|binding| binding.attempt_id.as_deref()),
            failure_kind: failure.kind,
            reason: &failure.reason,
        };
        log::warn!(
            "[runtime-plan-binding] tab={tab_id} stage={stage:?} kind={:?}: {}",
            failure.kind,
            failure.reason
        );
        let text = serde_json::to_string(&warning).unwrap_or_else(|_| failure.reason.clone());
        let recorded = if let Some(TaskBinding {
            plan_id: Some(plan),
            ..
        }) = binding.as_ref()
        {
            match self.plan_store.add_plan_stride(
                plan,
                "runtime_plan_binding_warning",
                &text,
                crate::plan::AgentRef::from_tab(tab_id),
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|elapsed| elapsed.as_millis() as u64)
                    .unwrap_or_default(),
            ) {
                Ok(_) => true,
                Err(error) => {
                    log::warn!(
                        "[runtime-plan-binding] tab={tab_id}: warning persistence failed: {error}"
                    );
                    false
                }
            }
        } else {
            false
        };
        let notice = (!recorded).then(|| {
            self.log_system_notice(
                "Runtime plan binding warning".into(),
                Some(text.clone()),
                Some("warning"),
            );
            text
        });
        self.emit_state_changed();
        notice
    }
}

#[cfg(test)]
mod tests {
    use crate::{
        multiplexer::{state::MultiplexerState, types::TaskBinding},
        service::test_fixture::ServiceFixture,
    };

    #[test]
    fn unavailable_plan_records_a_typed_notice_without_failing_finalization() {
        let (mut mux, tab_id) = MultiplexerState::new();
        mux.get_tab_mut(&tab_id).unwrap().task_binding = Some(TaskBinding {
            plan_id: Some("unavailable-plan".into()),
            task_id: Some("unavailable-task".into()),
            attempt_id: Some("attempt".into()),
            ..Default::default()
        });
        let fixture = ServiceFixture::new(mux);
        let Some(text) = fixture
            .service
            .report_runtime_worker_session_finalization(&tab_id, "thread", "nonce")
        else {
            panic!("an unavailable plan routes its warning to a system notice");
        };
        let notice: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(notice["kind"], "runtime_plan_binding_warning");
        assert_eq!(notice["plan_id"], "unavailable-plan");
        assert_eq!(notice["task_id"], "unavailable-task");
        assert_eq!(notice["attempt_id"], "attempt");
        assert_eq!(notice["stage"], "finalization");
        assert!(fixture.service.notif.lock().list(false).is_empty());
        assert!(fixture
            .service
            .finalize_runtime_worker_session(&tab_id, "thread", "nonce")
            .is_ok());
        assert!(fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .unwrap()
            .task_binding
            .is_some());
    }
}
