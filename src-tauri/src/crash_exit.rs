use codeterm_types::server_bootstrap::{
    AttachReply, AttachRequest, BootstrapOperation, ServerReady, BOOTSTRAP_PROTOCOL,
};
use std::sync::{Mutex, MutexGuard, OnceLock};

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientLifecycleEvent {
    MainWindowClose,
    ExplicitQuit,
    ClientOnlyUpdate,
    AuthorizedServerOrPairUpdate,
    UnixSignal,
    ClientCrashRelaunch,
    ServerUnavailable,
}

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientLifecycleAction {
    CloseWindowKeepOwners,
    ShutdownExactServerThenExitClient,
    ExitClientOnly,
    ReconnectToExistingServer,
    ShowReconnectingAndPreserveUi,
}

#[cfg(test)]
pub fn plan_client_lifecycle(event: ClientLifecycleEvent) -> ClientLifecycleAction {
    match event {
        ClientLifecycleEvent::MainWindowClose => ClientLifecycleAction::CloseWindowKeepOwners,
        ClientLifecycleEvent::ExplicitQuit => {
            ClientLifecycleAction::ShutdownExactServerThenExitClient
        }
        ClientLifecycleEvent::ClientOnlyUpdate | ClientLifecycleEvent::UnixSignal => {
            ClientLifecycleAction::ExitClientOnly
        }
        ClientLifecycleEvent::AuthorizedServerOrPairUpdate => {
            ClientLifecycleAction::ShutdownExactServerThenExitClient
        }
        ClientLifecycleEvent::ClientCrashRelaunch => {
            ClientLifecycleAction::ReconnectToExistingServer
        }
        ClientLifecycleEvent::ServerUnavailable => {
            ClientLifecycleAction::ShowReconnectingAndPreserveUi
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientExitReason {
    ExplicitQuit,
    ClientOnlyQuit,
    #[cfg(unix)]
    UnixSignal,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientExitPhase {
    Requested,
    Uncertain,
    Confirmed,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientExitStatus {
    pub operation_id: u64,
    pub reason: ClientExitReason,
    pub phase: ClientExitPhase,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientExitTicket {
    operation_id: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ClientExitRefusal {
    AlreadyInProgress,
}

struct ClientExitGate {
    state: Mutex<ClientExitGateState>,
    events: tokio::sync::watch::Sender<bool>,
}

impl Default for ClientExitGate {
    fn default() -> Self {
        Self {
            state: Mutex::new(ClientExitGateState::default()),
            events: tokio::sync::watch::channel(false).0,
        }
    }
}

#[derive(Default)]
struct ClientExitGateState {
    next_operation_id: u64,
    active: Option<ClientExitStatus>,
}

impl ClientExitGate {
    fn lock(&self) -> MutexGuard<'_, ClientExitGateState> {
        self.state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    fn begin(&self, reason: ClientExitReason) -> Result<ClientExitTicket, ClientExitRefusal> {
        let mut state = self.lock();
        if state.active.is_some() {
            return Err(ClientExitRefusal::AlreadyInProgress);
        }
        state.next_operation_id = state.next_operation_id.wrapping_add(1).max(1);
        let operation_id = state.next_operation_id;
        state.active = Some(ClientExitStatus {
            operation_id,
            reason,
            phase: ClientExitPhase::Requested,
        });
        self.events.send_replace(true);
        Ok(ClientExitTicket { operation_id })
    }

    fn mark_uncertain(&self, ticket: ClientExitTicket) -> bool {
        let mut state = self.lock();
        let Some(active) = state.active.as_mut() else {
            return false;
        };
        if active.operation_id != ticket.operation_id {
            return false;
        }
        match active.phase {
            ClientExitPhase::Requested | ClientExitPhase::Uncertain => {
                active.phase = ClientExitPhase::Uncertain;
                true
            }
            ClientExitPhase::Confirmed => false,
        }
    }

    fn confirm(&self, ticket: ClientExitTicket) -> bool {
        let mut state = self.lock();
        let Some(active) = state.active.as_mut() else {
            return false;
        };
        if active.operation_id != ticket.operation_id {
            return false;
        }
        active.phase = ClientExitPhase::Confirmed;
        true
    }

    // A Requested shutdown is still in flight and only its own outcome may resolve it.
    fn confirm_client_only(&self) -> bool {
        let mut state = self.lock();
        let confirmed = match state.active.as_mut() {
            None => {
                state.next_operation_id = state.next_operation_id.wrapping_add(1).max(1);
                state.active = Some(ClientExitStatus {
                    operation_id: state.next_operation_id,
                    reason: ClientExitReason::ClientOnlyQuit,
                    phase: ClientExitPhase::Confirmed,
                });
                true
            }
            Some(active) => match active.phase {
                ClientExitPhase::Uncertain | ClientExitPhase::Confirmed => {
                    active.phase = ClientExitPhase::Confirmed;
                    true
                }
                ClientExitPhase::Requested => false,
            },
        };
        if confirmed {
            self.events.send_replace(true);
        }
        confirmed
    }

    fn refuse_before_dispatch(&self, ticket: ClientExitTicket) -> bool {
        let mut state = self.lock();
        let Some(active) = state.active else {
            return false;
        };
        if active.operation_id != ticket.operation_id || active.phase != ClientExitPhase::Requested
        {
            return false;
        }
        state.active = None;
        self.events.send_replace(false);
        true
    }

    fn status(&self) -> Option<ClientExitStatus> {
        self.lock().active
    }
}

static CLIENT_EXIT_GATE: OnceLock<ClientExitGate> = OnceLock::new();
pub(crate) fn subscribe_client_exit() -> tokio::sync::watch::Receiver<bool> {
    client_exit_gate().events.subscribe()
}

fn client_exit_gate() -> &'static ClientExitGate {
    CLIENT_EXIT_GATE.get_or_init(ClientExitGate::default)
}

pub fn begin_client_exit(reason: ClientExitReason) -> Result<ClientExitTicket, ClientExitRefusal> {
    client_exit_gate().begin(reason)
}

pub fn mark_client_exit_uncertain(ticket: ClientExitTicket) -> bool {
    client_exit_gate().mark_uncertain(ticket)
}

pub fn confirm_client_exit(ticket: ClientExitTicket) -> bool {
    client_exit_gate().confirm(ticket)
}

pub fn confirm_client_only_exit() -> bool {
    client_exit_gate().confirm_client_only()
}

pub fn refuse_client_exit_before_dispatch(ticket: ClientExitTicket) -> bool {
    client_exit_gate().refuse_before_dispatch(ticket)
}

pub fn client_exit_status() -> Option<ClientExitStatus> {
    client_exit_gate().status()
}

pub fn is_client_exit_pending() -> bool {
    client_exit_status().is_some()
}

pub fn is_exact_server_shutdown_reply(
    expected: &ServerReady,
    request: &AttachRequest,
    reply: &AttachReply,
) -> bool {
    request.operation == BootstrapOperation::Shutdown
        && request.protocol == BOOTSTRAP_PROTOCOL
        && expected.protocol == BOOTSTRAP_PROTOCOL
        && request.api_version == expected.api_version
        && request.build == expected.build
        && request.identity == expected.identity
        && !request.challenge.is_empty()
        && request.challenge.len() <= 128
        && !expected.epoch.is_empty()
        && expected.process.pid != 0
        && request.expected.as_ref().is_some_and(|actual| {
            actual.epoch == expected.epoch && actual.process == expected.process
        })
        && reply.challenge == request.challenge
        && &reply.ready == expected
}

#[cfg(test)]
#[path = "crash_exit/tests.rs"]
mod tests;
