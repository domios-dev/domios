use super::store_db::hot_write::runtime::PlannerSessionRows;
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
        self.with_runtime_rows::<PlannerSessionRows, _>(plan_id, now_ms, |plan| {
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
mod tests;
