use super::*;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicU64, Ordering};

const MAX_SCOPES: usize = 256;

#[derive(Clone, Default)]
pub(crate) struct Observation {
    pub projection: Option<BTreeMap<String, String>>,
    pub workers: BTreeMap<String, Worker>,
    pub last_activity: BTreeMap<String, u64>,
    pub answered_choices: Vec<String>,
    pub pending_hard: Vec<HardWake>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Refusal {
    InFlight,
    Capacity,
}

impl Refusal {
    pub(crate) fn reason(self) -> &'static str {
        match self {
            Self::InFlight => "in_flight",
            Self::Capacity => "scope_capacity",
        }
    }
}

/// Phase counts per scope: suppression, emission attempt, emitter acceptance
/// and emitter failure are recorded separately and never inferred from each other.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct Telemetry {
    pub suppressed: u64,
    pub attempted: u64,
    pub accepted: u64,
    pub failed: u64,
    pub last_reason: Option<&'static str>,
}

struct Slot {
    identity: String,
    state: String,
    hard: Vec<HardWake>,
    decision: Decision,
    attempted: bool,
}

impl Slot {
    /// An accepted reminder moves the delta baseline; the same plan state with an
    /// empty delta and no new hard event is still the same observation.
    fn same(&self, hash: &str, state: &str, snapshot: &Snapshot) -> bool {
        self.identity == hash
            || (snapshot.delta.is_empty()
                && self.state == state
                && snapshot.hard.iter().all(|hard| self.hard.contains(hard)))
    }
}

#[derive(Default)]
struct Entry {
    epoch: u64,
    baseline: Observation,
    slots: BTreeMap<ReminderKind, Slot>,
    last_soft_class: Option<String>,
    last_soft_delivery: Option<u64>,
    in_flight: bool,
    telemetry: Telemetry,
}

#[derive(Default)]
pub(crate) struct Gate {
    entries: Mutex<HashMap<String, Entry>>,
    blocker_notices: Mutex<HashSet<String>>,
    epochs: AtomicU64,
    ticks: Mutex<VecDeque<DeliveryTick>>,
    bodies: Mutex<VecDeque<(String, String, String)>>,
}

pub(crate) struct Claim<'a> {
    gate: &'a Gate,
    key: String,
    epoch: u64,
}

impl Claim<'_> {
    pub(crate) fn key(&self) -> &str {
        &self.key
    }
}

impl Drop for Claim<'_> {
    fn drop(&mut self) {
        if let Some(entry) = self.gate.entries.lock().get_mut(&self.key) {
            if entry.epoch == self.epoch {
                entry.in_flight = false;
            }
        }
    }
}

impl Gate {
    pub(crate) fn remember_body(&self, tab: &str, plan: &str, text: &str) {
        let mut bodies = self.bodies.lock();
        bodies.retain(|(t, p, _)| t != tab || p != plan);
        bodies.push_back((tab.into(), plan.into(), text.into()));
        while bodies.len() > MAX_SCOPES {
            bodies.pop_front();
        }
    }

    pub(crate) fn last_body(&self, tab: &str, plan: &str) -> Option<String> {
        self.bodies
            .lock()
            .iter()
            .find(|(t, p, _)| t == tab && p == plan)
            .map(|(_, _, text)| text.clone())
    }

    pub(crate) fn claim_blocker_notice(&self, event_key: &str) -> bool {
        self.blocker_notices.lock().insert(event_key.to_string())
    }

    fn entry<'m>(
        &self,
        entries: &'m mut HashMap<String, Entry>,
        key: &str,
    ) -> Option<&'m mut Entry> {
        if !entries.contains_key(key) {
            if entries.len() >= MAX_SCOPES {
                return None;
            }
            let epoch = self.epochs.fetch_add(1, Ordering::Relaxed) + 1;
            entries.insert(
                key.into(),
                Entry {
                    epoch,
                    ..Default::default()
                },
            );
        }
        entries.get_mut(key)
    }

    pub(crate) fn claim(&self, key: &str) -> Result<Claim<'_>, Refusal> {
        let mut entries = self.entries.lock();
        let entry = self.entry(&mut entries, key).ok_or(Refusal::Capacity)?;
        if entry.in_flight {
            entry.telemetry.suppressed += 1;
            entry.telemetry.last_reason = Some(Refusal::InFlight.reason());
            return Err(Refusal::InFlight);
        }
        entry.in_flight = true;
        Ok(Claim {
            gate: self,
            key: key.into(),
            epoch: entry.epoch,
        })
    }

    /// Drops every scope outside the current eligible set; an in-flight claim
    /// on a dropped scope can no longer record an attempt.
    pub(crate) fn retain(&self, keys: &HashSet<String>) {
        self.entries.lock().retain(|key, _| keys.contains(key));
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
        let Some(entry) = self.entry(&mut entries, key) else {
            return;
        };
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
    ) -> Result<Decision, String> {
        self.decide_with_interval(key, snapshot, now, IDLE_NO_REPORT_SECS)
    }

    /// One rule verdict per observation identity and reminder kind.
    pub(crate) fn decide_with_interval(
        &self,
        key: &str,
        snapshot: &Snapshot,
        now: u64,
        interval: u64,
    ) -> Result<Decision, String> {
        let hash = snapshot.hash();
        let state_hash = snapshot.state_hash();
        let kind = snapshot.scope.kind;
        let state = {
            let mut entries = self.entries.lock();
            self.entry(&mut entries, key).map(|entry| {
                (
                    entry
                        .slots
                        .get(&kind)
                        .filter(|slot| slot.same(&hash, &state_hash, snapshot))
                        .map(|slot| slot.decision.clone()),
                    entry.last_soft_class.clone(),
                    entry.last_soft_delivery,
                )
            })
        };
        let Some((cached, soft_class, soft_delivered)) = state else {
            return Ok(suppressed(snapshot, "rule", Refusal::Capacity.reason()));
        };
        let mut decision = rule(
            snapshot,
            cached.is_some(),
            soft_class.as_deref(),
            soft_delivered,
            now,
            interval,
        );
        decision.observation_id = crate::plan::ids::short_id("hb");
        if cached.is_some() {
            return Ok(decision);
        }
        let mut entries = self.entries.lock();
        if let Some(entry) = self.entry(&mut entries, key) {
            entry.slots.insert(
                kind,
                Slot {
                    identity: hash,
                    state: state_hash,
                    hard: snapshot.hard.clone(),
                    decision: decision.clone(),
                    attempted: false,
                },
            );
        }
        Ok(decision)
    }

    /// The single emission attempt for an approving observation; false when the
    /// scope was dropped, re-decided or already attempted.
    pub(crate) fn attempt(&self, claim: &Claim<'_>, decision: &Decision) -> bool {
        let mut entries = self.entries.lock();
        let Some(entry) = entries
            .get_mut(&claim.key)
            .filter(|entry| entry.epoch == claim.epoch)
        else {
            return false;
        };
        let Some(slot) = entry.slots.get_mut(&decision.scope.kind).filter(|slot| {
            slot.identity == decision.snapshot_hash && slot.decision.wake && !slot.attempted
        }) else {
            return false;
        };
        slot.attempted = true;
        entry.telemetry.attempted += 1;
        true
    }

    pub(crate) fn delivered(&self, key: &str, snapshot: &Snapshot, decision: &Decision, now: u64) {
        let mut entries = self.entries.lock();
        let Some(entry) = self.entry(&mut entries, key) else {
            return;
        };
        entry.baseline.projection = Some(snapshot.projection.clone());
        if snapshot.hard.is_empty() {
            entry.last_soft_class = Some(decision.soft_class());
            entry.last_soft_delivery = Some(now);
        }
        entry.baseline.pending_hard.clear();
        entry.telemetry.accepted += 1;
    }

    pub(crate) fn failed(&self, key: &str) {
        if let Some(entry) = self.entries.lock().get_mut(key) {
            entry.telemetry.failed += 1;
        }
    }

    pub(crate) fn suppressed(&self, key: &str, reason: &'static str) {
        if let Some(entry) = self.entries.lock().get_mut(key) {
            entry.telemetry.suppressed += 1;
            entry.telemetry.last_reason = Some(reason);
        }
    }

    pub(crate) fn telemetry(&self, key: &str) -> Telemetry {
        self.entries
            .lock()
            .get(key)
            .map(|entry| entry.telemetry.clone())
            .unwrap_or_default()
    }

    pub(crate) fn record_tick(&self, tick: DeliveryTick) {
        super::delivery_log::push_tick(&mut self.ticks.lock(), tick);
    }

    pub(crate) fn ticks_for_tab(&self, tab_id: &str) -> Vec<DeliveryTick> {
        self.ticks
            .lock()
            .iter()
            .filter(|tick| tick.tab_id == tab_id)
            .cloned()
            .collect()
    }

    #[cfg(test)]
    pub(crate) fn unlocked(&self) -> bool {
        self.entries.try_lock().is_some()
    }

    #[cfg(test)]
    pub(crate) fn scopes(&self) -> usize {
        self.entries.lock().len()
    }
}

pub(super) fn rule(
    snapshot: &Snapshot,
    same: bool,
    soft_class: Option<&str>,
    delivered: Option<u64>,
    now: u64,
    interval: u64,
) -> Decision {
    if same {
        return suppressed(snapshot, "rule", "same_snapshot");
    }
    let mut decision = cadence(snapshot);
    if decision.wake
        && snapshot.hard.is_empty()
        && soft_class == Some(decision.soft_class().as_str())
        && delivered.is_some_and(|last| now.saturating_sub(last) < interval)
    {
        decision.wake = false;
        decision.reason = "soft_cooldown";
    }
    decision
}
