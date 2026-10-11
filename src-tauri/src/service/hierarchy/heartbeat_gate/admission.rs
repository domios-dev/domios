use super::{HardWake, Snapshot};
use std::collections::BTreeSet;

#[derive(Clone, Default)]
pub(super) struct Admission {
    last_judgement: Option<u64>,
    events: BTreeSet<String>,
}

pub(super) fn admit(
    previous: &Admission,
    snapshot: &Snapshot,
    same_snapshot: bool,
    now: u64,
    interval: u64,
) -> (Admission, Result<(), &'static str>) {
    let events = actionable_events(snapshot);
    let actionable = !events.is_subset(&previous.events);
    let mut next = Admission {
        events,
        ..previous.clone()
    };
    let result = if same_snapshot {
        Err("same_snapshot")
    } else if !actionable
        && previous
            .last_judgement
            .is_some_and(|last| now.saturating_sub(last) < interval)
    {
        Err("judgement_cadence")
    } else {
        next.last_judgement = Some(now);
        Ok(())
    };
    (next, result)
}

fn actionable_events(snapshot: &Snapshot) -> BTreeSet<String> {
    let mut events = BTreeSet::new();
    for event in &snapshot.hard {
        let actionable = !matches!(event, HardWake::TaskState(_) | HardWake::ModeChange);
        if actionable {
            events.insert(serde_json::to_string(event).unwrap_or_default());
        }
    }
    for worker in &snapshot.workers {
        if let Some(prompt) = &worker.prompt {
            events.insert(format!("prompt:{}:{prompt}", worker.tab_id));
        }
    }
    events
}

#[cfg(test)]
mod tests;
