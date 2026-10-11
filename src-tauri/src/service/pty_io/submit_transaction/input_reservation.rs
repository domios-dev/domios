use std::collections::HashMap;
use std::sync::{Arc, OnceLock, Weak};

use parking_lot::{Condvar, Mutex};

#[derive(Default)]
struct InputState {
    sends: usize,
    command: bool,
    #[cfg(test)]
    waiting_sends: usize,
}

#[derive(Debug, PartialEq, Eq)]
enum Admission {
    Send,
    Command,
    Wait,
    Refuse,
}

fn admission(sends: usize, command: bool, wants_command: bool) -> Admission {
    match (wants_command, command, sends) {
        (true, false, 0) => Admission::Command,
        (true, _, _) => Admission::Refuse,
        (false, true, _) => Admission::Wait,
        (false, false, _) => Admission::Send,
    }
}

#[derive(Default)]
struct InputOwner {
    state: Mutex<InputState>,
    changed: Condvar,
}

pub(crate) struct InputReservation {
    owner: Arc<InputOwner>,
    command: bool,
}

impl InputOwner {
    fn send(self: &Arc<Self>) -> InputReservation {
        let mut state = self.state.lock();
        while admission(state.sends, state.command, false) == Admission::Wait {
            log::debug!("[pty-input] send waits for live command completion");
            #[cfg(test)]
            {
                state.waiting_sends += 1;
                self.changed.notify_all();
            }
            self.changed.wait(&mut state);
            #[cfg(test)]
            {
                state.waiting_sends -= 1;
            }
        }
        state.sends += 1;
        InputReservation {
            owner: self.clone(),
            command: false,
        }
    }

    fn command(self: &Arc<Self>) -> Result<InputReservation, &'static str> {
        let mut state = self.state.lock();
        if admission(state.sends, state.command, true) == Admission::Refuse {
            return Err("a send or model command is in flight; switch at its next idle prompt");
        }
        state.command = true;
        Ok(InputReservation {
            owner: self.clone(),
            command: true,
        })
    }
}

impl Drop for InputReservation {
    fn drop(&mut self) {
        let mut state = self.owner.state.lock();
        if self.command {
            state.command = false;
        } else {
            state.sends -= 1;
        }
        self.owner.changed.notify_all();
    }
}

fn owner(pty_id: &str) -> Arc<InputOwner> {
    static OWNERS: OnceLock<Mutex<HashMap<String, Weak<InputOwner>>>> = OnceLock::new();
    let mut owners = OWNERS.get_or_init(Mutex::default).lock();
    if let Some(owner) = owners.get(pty_id).and_then(Weak::upgrade) {
        return owner;
    }
    owners.retain(|_, owner| owner.strong_count() > 0);
    let owner = Arc::new(InputOwner::default());
    owners.insert(pty_id.into(), Arc::downgrade(&owner));
    owner
}

pub(super) fn reserve_send_input(pty_id: &str) -> InputReservation {
    owner(pty_id).send()
}

pub(crate) fn reserve_command_input(pty_id: &str) -> Result<InputReservation, &'static str> {
    owner(pty_id).command()
}

#[cfg(test)]
mod tests;
