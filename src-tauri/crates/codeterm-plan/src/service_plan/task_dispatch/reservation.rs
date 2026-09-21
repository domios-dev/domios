use super::super::{PlanError, PlanStore};
use super::{current_dispatch_startup_owner, DispatchStartupOwner};

#[derive(Debug, Clone)]
pub(crate) struct DispatchReservationOwner {
    attempt_id: String,
    tab_id: String,
}

pub struct TaskDispatchReservation<'a> {
    store: &'a PlanStore,
    key: (String, String),
    attempt_id: String,
}

impl Drop for TaskDispatchReservation<'_> {
    fn drop(&mut self) {
        let mut reservations = self.store.dispatch_reservations.lock();
        if reservations
            .get(&self.key)
            .is_some_and(|owner| owner.attempt_id == self.attempt_id)
        {
            reservations.remove(&self.key);
        }
    }
}

impl PlanStore {
    pub fn task_dispatch_startup_reservation_owns(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        tab_id: &str,
    ) -> bool {
        self.dispatch_reservations
            .lock()
            .get(&(plan_id.to_string(), task_id.to_string()))
            .is_some_and(|owner| owner.attempt_id == attempt_id && owner.tab_id == tab_id)
    }

    pub fn reserve_task_dispatch_startup(
        &self,
        plan_id: &str,
        task_id: &str,
        attempt_id: &str,
        tab_id: &str,
        observed_startup_owner: Option<DispatchStartupOwner>,
        observed_startup_owner_is_live: bool,
    ) -> Result<TaskDispatchReservation<'_>, PlanError> {
        let _writer_turn = self.mutation_writer.enter();
        let current_owner = self.get(plan_id).and_then(|plan| {
            crate::plan::task_key::find_task(&plan.tasks, task_id)
                .and_then(current_dispatch_startup_owner)
        });
        if current_owner.as_ref().is_some_and(|owner| {
            observed_startup_owner.as_ref() != Some(owner) || observed_startup_owner_is_live
        }) {
            let owner = current_owner.expect("changed startup owner must be present");
            return Err(PlanError::DispatchInFlight {
                attempt_id: owner.attempt_id,
                tab_id: owner.tab_id,
            });
        }
        let key = (plan_id.to_string(), task_id.to_string());
        let mut reservations = self.dispatch_reservations.lock();
        if let Some(owner) = reservations.get(&key) {
            return Err(PlanError::DispatchInFlight {
                attempt_id: owner.attempt_id.clone(),
                tab_id: owner.tab_id.clone(),
            });
        }
        reservations.insert(
            key.clone(),
            DispatchReservationOwner {
                attempt_id: attempt_id.to_string(),
                tab_id: tab_id.to_string(),
            },
        );
        drop(reservations);
        drop(_writer_turn);
        Ok(TaskDispatchReservation {
            store: self,
            key,
            attempt_id: attempt_id.to_string(),
        })
    }
}
