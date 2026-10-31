use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeldReason {
    PlanLimitLocked,
}

impl HeldReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PlanLimitLocked => "plan_limit_locked",
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Admission {
    pub held_reason: Option<HeldReason>,
    pub baseline_pending: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub continuation: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub plugin_hold: Option<PluginHold>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginHold {
    pub run: String,
    pub node: String,
    pub operation: String,
    pub state: String,
    pub reason: String,
}

impl Admission {
    pub fn reconcile(&mut self, locked: bool, watch: bool) -> bool {
        let before = self.clone();
        if locked {
            self.held_reason = Some(HeldReason::PlanLimitLocked);
            self.baseline_pending |= watch;
        } else {
            self.held_reason = None;
        }
        *self != before
    }

    pub fn permits_steps(&self) -> bool {
        self.held_reason.is_none() && self.plugin_hold.is_none()
    }
}

impl crate::store::Store {
    pub fn stage_admission(
        &mut self,
        id: &str,
        admission: Admission,
    ) -> crate::storage::Staged<bool> {
        use crate::storage::{finishing, Failure, Staged};
        if let Err(error) = self.ensure_available() {
            return Staged::ready(Err(error));
        }
        let Some(job) = self.get(id).cloned() else {
            return Staged::ready(Ok(false));
        };
        if job.admission == admission {
            return Staged::ready(Ok(false));
        }
        let advance_generation = job.pipeline().is_none() || !self.active.contains_key(id);
        if self.backend.is_json() {
            if let Some(current) = self.jobs.iter_mut().find(|current| current.id == id) {
                current.admission = admission;
                current.revision += u64::from(advance_generation);
            }
            return Staged::ready(self.save_legacy().map(|()| true));
        }
        let stored = match crate::storage::codec::id(id) {
            Ok(id) => id,
            Err(error) => return Staged::ready(Err(error)),
        };
        let json = serde_json::json!(admission).to_string();
        self.submit(
            move |repo| repo.set_admission(stored, job.revision, advance_generation, json),
            finishing(move |_, result| {
                result
                    .map(|commit| commit.outcome == codeterm_storage::MutationOutcome::Applied)
                    .map_err(Failure::message)
            }),
        )
    }
}

#[cfg(test)]
mod tests;
