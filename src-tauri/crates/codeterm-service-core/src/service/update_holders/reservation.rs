use super::*;
use crate::pty::write_queue::UpdateWriteReservation;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

const RESERVED: &str =
    "tab_busy_refused: update shutdown has reserved input; wait for the update outcome";

struct Reservation {
    id: String,
    _writes: Vec<UpdateWriteReservation>,
}

#[derive(Default)]
struct GateState {
    reservation: Option<Reservation>,
    active: HashMap<String, usize>,
    retiring: HashSet<String>,
}

#[derive(Default)]
struct Gate(Arc<Mutex<GateState>>);

struct ReservationAttempt {
    gate: Arc<Mutex<GateState>>,
    id: String,
    committed: bool,
}

impl Drop for ReservationAttempt {
    fn drop(&mut self) {
        if !self.committed {
            let mut state = self.gate.lock();
            if state
                .reservation
                .as_ref()
                .is_some_and(|reservation| reservation.id == self.id)
            {
                state.reservation = None;
            }
        }
    }
}

pub(crate) struct ActiveUpdateInput {
    gate: Arc<Mutex<GateState>>,
    tab: String,
}

pub struct PtyInputRetirement {
    gate: Arc<Mutex<GateState>>,
    tab: String,
}

impl Drop for PtyInputRetirement {
    fn drop(&mut self) {
        self.gate.lock().retiring.remove(&self.tab);
    }
}

impl Drop for ActiveUpdateInput {
    fn drop(&mut self) {
        let mut gate = self.gate.lock();
        if let Some(count) = gate.active.get_mut(&self.tab) {
            *count -= 1;
            if *count == 0 {
                gate.active.remove(&self.tab);
            }
        }
    }
}

impl super::super::ServiceContext {
    pub fn reserve_pty_input_retirement(
        &self,
        claim: &crate::service::TabPtyClaim,
    ) -> Result<PtyInputRetirement, String> {
        let tab = claim.tab_id();
        let gate = self.ext_or_init(Gate::default).0.clone();
        let mut state = gate.lock();
        if state.reservation.is_some() {
            return Err(RESERVED.into());
        }
        if state.active.contains_key(tab) || !state.retiring.insert(tab.into()) {
            return Err("Input is already in progress on the selected account".into());
        }
        drop(state);
        Ok(PtyInputRetirement {
            gate,
            tab: tab.into(),
        })
    }

    pub(crate) fn admit_update_input(&self, tab_id: &str) -> Result<ActiveUpdateInput, String> {
        let gate = self.ext_or_init(Gate::default).0.clone();
        let mut state = gate.lock();
        if state.reservation.is_some() {
            return Err(RESERVED.into());
        }
        if state.retiring.contains(tab_id) {
            return Err("tab_busy_refused: the PTY is being retired for account rollback".into());
        }
        if let Some(pty_id) = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.pty_id.clone())
        {
            if let Some(session) = self.pty.lock().get_session(&pty_id) {
                session.write_queue.admit_work()?;
            }
        }
        *state.active.entry(tab_id.into()).or_default() += 1;
        drop(state);
        Ok(ActiveUpdateInput {
            gate,
            tab: tab_id.into(),
        })
    }

    pub(crate) fn admit_update_pty_input(&self, pty_id: &str) -> Result<ActiveUpdateInput, String> {
        let tab = self
            .mux
            .read()
            .tab_id_for_pty(pty_id)
            .unwrap_or_else(|| pty_id.into());
        self.admit_update_input(&tab)
    }

    pub fn reserve_update_shutdown(
        &self,
        request: codeterm_update::holders::ReserveHolders,
    ) -> Result<String, String> {
        let gate = self.ext_or_init(Gate::default).0.clone();
        reserve_with(
            &gate,
            &request.tabs,
            request.initiating_tab.as_deref(),
            |expected| {
                if self.update_holder_tab(&expected.process)?.as_deref()
                    != Some(expected.tab_id.as_str())
                {
                    return Err("holder process identity or PTY ownership changed".into());
                }
                let mux = self.mux.read();
                let tab = mux
                    .get_tab(&expected.tab_id)
                    .ok_or("holder tab disappeared")?;
                if tab.pty_id.as_deref() != Some(expected.pty_id.as_str()) {
                    return Err("holder PTY changed".into());
                }
                let session = self
                    .pty
                    .lock()
                    .get_session(&expected.pty_id)
                    .ok_or("holder PTY disappeared")?;
                if session.generation != expected.pty_generation {
                    return Err("holder PTY generation changed".into());
                }
                session
                    .write_queue
                    .reserve_update_shutdown(expected.input_revision)
            },
            || {
                let mut actual = Vec::new();
                for expected in &request.tabs {
                    actual.extend(
                        self.describe_update_holders(vec![expected.process.clone()])
                            .map_err(|reason| changed(expected, &reason))?,
                    );
                }
                Ok(actual)
            },
        )
    }

    pub fn release_update_shutdown(&self, id: &str) -> Result<(), String> {
        let gate = self.ext_or_init(Gate::default).0.clone();
        let mut state = gate.lock();
        if state
            .reservation
            .as_ref()
            .is_none_or(|reservation| reservation.id != id)
        {
            return Err("update shutdown reservation identity changed".into());
        }
        state.reservation = None;
        Ok(())
    }
}

fn changed(tab: &HolderTab, reason: &str) -> String {
    let mut blocker = tab.clone();
    blocker.busy = true;
    blocker.command = format!("{} ({reason})", blocker.command);
    codeterm_update::holders::refusal(&[blocker]).unwrap_or_else(|| reason.into())
}

fn reserve_with(
    gate: &Arc<Mutex<GateState>>,
    expected: &[HolderTab],
    initiating: Option<&str>,
    mut freeze: impl FnMut(&HolderTab) -> Result<UpdateWriteReservation, String>,
    validate: impl FnOnce() -> Result<Vec<HolderTab>, String>,
) -> Result<String, String> {
    let id = uuid::Uuid::new_v4().to_string();
    {
        let mut state = gate.lock();
        if state.reservation.is_some() {
            return Err(RESERVED.into());
        }
        for tab in expected {
            if state.retiring.contains(&tab.tab_id) {
                return Err(changed(tab, "PTY retirement is in progress"));
            }
            if Some(tab.tab_id.as_str()) != initiating && state.active.contains_key(&tab.tab_id) {
                return Err(changed(tab, "input admission is in flight"));
            }
        }
        state.reservation = Some(Reservation {
            id: id.clone(),
            _writes: Vec::new(),
        });
    }
    let mut attempt = ReservationAttempt {
        gate: gate.clone(),
        id: id.clone(),
        committed: false,
    };
    let mut writes = Vec::new();
    let mut seen = std::collections::HashSet::new();
    for tab in expected {
        if !seen.insert(tab.pty_id.clone()) {
            continue;
        }
        writes.push(freeze(tab).map_err(|reason| changed(tab, &reason))?);
    }
    let actual = validate()?;
    for tab in expected {
        if !actual.iter().any(|current| {
            current.tab_id == tab.tab_id
                && current.process.pid == tab.process.pid
                && current.process.created == tab.process.created
                && current.pty_id == tab.pty_id
                && current.pty_generation == tab.pty_generation
        }) {
            return Err(changed(tab, "holder identity changed during reservation"));
        }
    }
    let blocking: Vec<_> = actual
        .into_iter()
        .filter(|tab| Some(tab.tab_id.as_str()) != initiating)
        .collect();
    if let Some(refusal) = codeterm_update::holders::refusal(&blocking) {
        return Err(refusal);
    }
    let mut state = gate.lock();
    let reservation = state
        .reservation
        .as_mut()
        .filter(|reservation| reservation.id == id)
        .ok_or("update shutdown reservation was cancelled")?;
    reservation._writes = writes;
    attempt.committed = true;
    Ok(id)
}

#[cfg(test)]
mod tests;
