use std::collections::{HashMap, HashSet};
use std::sync::{Arc, OnceLock, Weak};
use std::time::Instant;

use parking_lot::Mutex;
use tokio::sync::watch;

#[derive(Default)]
struct InputState {
    sends: usize,
    command: Option<CommandWait>,
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

#[derive(Clone)]
pub(crate) struct CommandWait {
    completion: watch::Receiver<bool>,
    signal: watch::Sender<bool>,
    deadline: Instant,
    scheduled_users: Arc<Mutex<HashSet<String>>>,
}

impl CommandWait {
    pub(crate) fn schedule_user(&self, send_id: &str) -> bool {
        self.scheduled_users.lock().insert(send_id.to_string())
    }

    pub(crate) async fn completed(mut self) -> bool {
        let deadline = self.deadline;
        let completion = async {
            loop {
                if *self.completion.borrow_and_update() {
                    return true;
                }
                if self.completion.changed().await.is_err() {
                    return false;
                }
            }
        };
        tokio::time::timeout_at(deadline.into(), completion)
            .await
            .unwrap_or(false)
    }
}

#[derive(Default)]
struct InputOwner {
    state: Mutex<InputState>,
}

pub(crate) struct InputReservation {
    owner: Arc<InputOwner>,
    completion: Option<watch::Sender<bool>>,
    on_completion: Option<Box<dyn FnOnce() + Send + Sync>>,
}

impl InputOwner {
    fn send(self: &Arc<Self>) -> Result<InputReservation, CommandWait> {
        let mut state = self.state.lock();
        if let Some(command) = state
            .command
            .as_ref()
            .filter(|_| admission(state.sends, state.command.is_some(), false) == Admission::Wait)
        {
            return Err(command.clone());
        }
        state.sends += 1;
        Ok(InputReservation {
            owner: self.clone(),
            completion: None,
            on_completion: None,
        })
    }

    fn command(self: &Arc<Self>, deadline: Instant) -> Result<InputReservation, &'static str> {
        let mut state = self.state.lock();
        if admission(state.sends, state.command.is_some(), true) == Admission::Refuse {
            return Err("a send or model command is in flight; switch at its next idle prompt");
        }
        let (completion, receiver) = watch::channel(false);
        state.command = Some(CommandWait {
            completion: receiver,
            signal: completion.clone(),
            deadline,
            scheduled_users: Arc::new(Mutex::new(HashSet::new())),
        });
        Ok(InputReservation {
            owner: self.clone(),
            completion: Some(completion),
            on_completion: None,
        })
    }
}

impl InputReservation {
    pub(crate) fn on_completion(mut self, notify: impl FnOnce() + Send + Sync + 'static) -> Self {
        self.on_completion = Some(Box::new(notify));
        self
    }
}

impl Drop for InputReservation {
    fn drop(&mut self) {
        let mut state = self.owner.state.lock();
        if let Some(completion) = &self.completion {
            if state
                .command
                .as_ref()
                .is_some_and(|command| completion.same_channel(&command.signal))
            {
                state.command = None;
            }
            completion.send_replace(true);
        } else {
            state.sends -= 1;
        }
        drop(state);
        if let Some(notify) = self.on_completion.take() {
            notify();
        }
    }
}

fn owners() -> &'static Mutex<HashMap<String, Weak<InputOwner>>> {
    static OWNERS: OnceLock<Mutex<HashMap<String, Weak<InputOwner>>>> = OnceLock::new();
    OWNERS.get_or_init(Mutex::default)
}

fn owner(pty_id: &str) -> Arc<InputOwner> {
    let mut owners = owners().lock();
    if let Some(owner) = owners.get(pty_id).and_then(Weak::upgrade) {
        return owner;
    }
    owners.retain(|_, owner| owner.strong_count() > 0);
    let owner = Arc::new(InputOwner::default());
    owners.insert(pty_id.into(), Arc::downgrade(&owner));
    owner
}

pub(super) fn reserve_send_input(pty_id: &str) -> Result<InputReservation, CommandWait> {
    owner(pty_id).send()
}

pub(crate) fn reserve_command_input(
    pty_id: &str,
    deadline: Instant,
) -> Result<InputReservation, &'static str> {
    owner(pty_id).command(deadline)
}

pub(super) fn command_pty_exited(pty_id: &str) {
    let owner = owners().lock().get(pty_id).and_then(Weak::upgrade);
    if let Some(owner) = owner {
        let mut state = owner.state.lock();
        if let Some(command) = state.command.take() {
            // PTY exit ends the reservation; subsequent delivery still revalidates authority.
            command.signal.send_replace(true);
        }
    }
}

#[cfg(test)]
mod tests;
mod user_wait;
