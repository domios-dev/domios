use super::*;

#[derive(Clone, Default)]
pub(crate) struct Observation {
    pub projection: Option<BTreeMap<String, String>>,
    pub workers: BTreeMap<String, Worker>,
    pub last_activity: BTreeMap<String, u64>,
    pub answered_choices: Vec<String>,
    pub pending_hard: Vec<HardWake>,
}

#[derive(Default)]
struct Entry {
    baseline: Observation,
    first_tick: Option<u64>,
    last_delivered: Option<u64>,
    last_decision: Option<Decision>,
    last_hash: Option<String>,
    delivery_pending: bool,
    last_soft_class: Option<String>,
    last_soft_delivery: Option<u64>,
    in_flight: bool,
}

#[derive(Default)]
pub(crate) struct Gate {
    entries: Mutex<HashMap<String, Entry>>,
}

pub(crate) struct Claim<'a> {
    gate: &'a Gate,
    key: String,
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.gate.entries.lock().get_mut(&self.key) {
            entry.in_flight = false;
        }
    }
}

impl Gate {
    pub(crate) fn claim(&self, key: &str) -> Option<Claim<'_>> {
        let mut entries = self.entries.lock();
        let entry = entries.entry(key.into()).or_default();
        if entry.in_flight {
            return None;
        }
        entry.in_flight = true;
        Some(Claim {
            gate: self,
            key: key.into(),
        })
    }

    pub(crate) fn mode_changed(&self, key: &str, current: &str) -> bool {
        self.entries
            .lock()
            .get(key)
            .and_then(|entry| entry.baseline.projection.as_ref())
            .and_then(|projection| projection.get("mode"))
            .is_some_and(|mode| mode != current)
    }

    pub(crate) fn observation(&self, key: &str) -> Observation {
        self.entries
            .lock()
            .get(key)
            .map(|entry| entry.baseline.clone())
            .unwrap_or_default()
    }

    pub(crate) fn observed(&self, key: &str, observation: Observation) {
        let mut entries = self.entries.lock();
        let entry = entries.entry(key.into()).or_default();
        if entry.baseline.projection.is_none() {
            entry.baseline.projection = observation.projection;
        }
        entry.baseline.workers = observation.workers;
        entry.baseline.last_activity = observation.last_activity;
        entry.baseline.answered_choices = observation.answered_choices;
        entry.baseline.pending_hard = observation.pending_hard;
    }

    #[cfg(test)]
    pub(crate) fn decide(
        &self,
        key: &str,
        snapshot: &Snapshot,
        now: u64,
        model: Option<&dyn DecisionModel>,
        deadline: Instant,
    ) -> Result<Decision, String> {
        self.decide_with_interval(key, snapshot, now, model, deadline, IDLE_NO_REPORT_SECS)
    }

    pub(crate) fn decide_with_interval(
        &self,
        key: &str,
        snapshot: &Snapshot,
        now: u64,
        model: Option<&dyn DecisionModel>,
        deadline: Instant,
        interval: u64,
    ) -> Result<Decision, String> {
        let hash = snapshot.hash();
        let (same, cached, pending, silence, soft_class, soft_delivered) = {
            let mut entries = self.entries.lock();
            let entry = entries.entry(key.into()).or_default();
            let first = *entry.first_tick.get_or_insert(now);
            (
                entry.last_hash.as_deref() == Some(&hash),
                entry.last_decision.clone(),
                entry.delivery_pending,
                now.saturating_sub(entry.last_delivered.unwrap_or(first)) >= SAFETY_STATUS_SECS,
                entry.last_soft_class.clone(),
                entry.last_soft_delivery,
            )
        };
        let hard = !snapshot.hard.is_empty();
        let mut decision = if same {
            let mut decision = cached.ok_or("heartbeat decision cache unavailable")?;
            decision.wake = pending || hard || silence;
            decision.safety = decision.safety && pending;
            decision.reason = if hard { "hard_wake" } else { "same_snapshot" };
            decision.decided_by = "heuristic".into();
            if !decision.safety {
                decision.next_steps = snapshot
                    .hard
                    .iter()
                    .map(|hard| hard.next_step(&snapshot.plan_id))
                    .collect();
                for worker in &snapshot.workers {
                    let label = decision
                        .worker_labels
                        .get(&worker.tab_id)
                        .map(String::as_str)
                        .unwrap_or_else(|| worker.heuristic_label());
                    if worker.unhealthy()
                        || !matches!(label, "on_track" | "idle_no_report" | "looping")
                        || (!worker.held && matches!(label, "idle_no_report" | "looping"))
                    {
                        decision.next_steps.push(worker.next_step(label));
                    }
                }
                if decision.wake && decision.next_steps.is_empty() {
                    decision.next_steps.push(format!(
                        "inspect the pending decision with codeterm plan state {}",
                        snapshot.plan_id
                    ));
                }
            }
            decision
        } else if model.is_some()
            && !hard
            && snapshot.delta.is_empty()
            && snapshot.workers.iter().all(|worker| !worker.unhealthy())
        {
            fallback(snapshot, "heuristic", "healthy_snapshot")
        } else {
            match model {
                Some(model) => classify(snapshot, model, deadline).unwrap_or_else(|error| {
                    let reason = match error {
                        DecisionError::Budget => "budget_exhausted",
                        DecisionError::Deadline => "model_deadline",
                        _ => "model_failed",
                    };
                    fallback(snapshot, &format!("fallback({reason})"), reason)
                }),
                None => fallback(snapshot, "fallback(no_adapter)", "no_adapter"),
            }
        };
        if decision.wake
            && !hard
            && !pending
            && !decision.safety
            && soft_class.as_deref() == Some(decision.soft_class().as_str())
            && soft_delivered.is_some_and(|last| now.saturating_sub(last) < interval)
        {
            decision.wake = false;
            decision.reason = "soft_cooldown";
        }
        if silence && !decision.wake {
            decision.wake = true;
            decision.safety = true;
            decision.reason = "safety_status";
        }
        if same && silence && !pending && !hard {
            decision.safety = true;
            decision.reason = "safety_status";
        }
        decision.observation_id = crate::plan::ids::short_id("hb");
        let mut entries = self.entries.lock();
        let entry = entries.entry(key.into()).or_default();
        entry.last_hash = Some(hash);
        entry.last_decision = Some(decision.clone());
        entry.delivery_pending = decision.wake;
        Ok(decision)
    }

    pub(crate) fn delivered(&self, key: &str, snapshot: &Snapshot, decision: &Decision, now: u64) {
        let mut entries = self.entries.lock();
        let entry = entries.entry(key.into()).or_default();
        entry.baseline.projection = Some(snapshot.projection.clone());
        entry.last_delivered = Some(now);
        if snapshot.hard.is_empty() && !decision.safety {
            entry.last_soft_class = Some(decision.soft_class());
            entry.last_soft_delivery = Some(now);
        }
        entry.delivery_pending = false;
        entry.baseline.pending_hard.clear();
    }
}
