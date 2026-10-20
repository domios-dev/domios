use super::{PlanError, PlanStore};
use crate::plan::PlannerRef;

impl PlanStore {
    pub fn complete_planner_report_session(
        &self,
        plan_id: &str,
        tab_id: &str,
        session_id: &str,
        now_ms: u64,
    ) -> Result<(), PlanError> {
        if session_id.trim().is_empty() {
            return Err(PlanError::Io("Planner report session is empty".into()));
        }
        self.with_plan_if_changed(plan_id, now_ms, |plan| {
            let Some(PlannerRef::Tab {
                tab_id: registered_tab,
                session_id: registered_session,
                ..
            }) = &mut plan.planner
            else {
                return Err(PlanError::Io("Planner report registration changed".into()));
            };
            if registered_tab.as_str() != tab_id
                || registered_session
                    .as_deref()
                    .is_some_and(|registered| registered != session_id)
            {
                return Err(PlanError::Io("Planner report registration changed".into()));
            }
            if registered_session.is_some() {
                return Ok(None);
            }
            *registered_session = Some(session_id.into());
            Ok(Some(()))
        })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::plan::{OrchestratorRef, Plan};

    #[test]
    fn planner_report_session_completion_preserves_registration_and_refuses_foreign_identity() {
        let dir = tempfile::tempdir().unwrap();
        let store = PlanStore::new_for_test(dir.path());
        let mut plan = Plan::new(
            "planner".into(),
            dir.path().display().to_string(),
            OrchestratorRef::from_tab("owner"),
        );
        plan.planner = Some(PlannerRef::Tab {
            tab_id: "planner".into(),
            session_id: None,
            title: Some("Euclid".into()),
        });
        store.save(&mut plan, 1).unwrap();
        let before = store.get(&plan.id).unwrap();
        assert!(store
            .complete_planner_report_session(&plan.id, "foreign", "session", 2)
            .is_err());
        assert_eq!(store.get(&plan.id).unwrap(), before);
        store
            .complete_planner_report_session(&plan.id, "planner", "session", 3)
            .unwrap();
        let after = store.get(&plan.id).unwrap();
        assert_eq!(
            after.planner,
            Some(PlannerRef::Tab {
                tab_id: "planner".into(),
                session_id: Some("session".into()),
                title: Some("Euclid".into())
            })
        );
        store
            .complete_planner_report_session(&plan.id, "planner", "session", 4)
            .unwrap();
        assert_eq!(store.get(&plan.id).unwrap(), after);
        assert!(store
            .complete_planner_report_session(&plan.id, "planner", "foreign-session", 5)
            .is_err());
        assert_eq!(store.get(&plan.id).unwrap(), after);
    }
}
