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
    #[cfg(test)]
    ClientOnlyUpdate,
    #[cfg(test)]
    AuthorizedServerOrPairUpdate,
    #[cfg(any(unix, test))]
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

#[derive(Default)]
struct ClientExitGate {
    state: Mutex<ClientExitGateState>,
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
        true
    }

    fn status(&self) -> Option<ClientExitStatus> {
        self.lock().active
    }
}

static CLIENT_EXIT_GATE: OnceLock<ClientExitGate> = OnceLock::new();

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
mod tests {
    use super::*;
    use codeterm_types::server_bootstrap::{ProcessProvenance, ServerExpectation, ServerIdentity};
    use std::{net::SocketAddr, path::PathBuf};

    fn ready(epoch: &str, pid: u32) -> ServerReady {
        ServerReady {
            protocol: BOOTSTRAP_PROTOCOL,
            api_version: 3,
            build: "server-build".to_string(),
            instance_id: "split-oracle-quit".to_string(),
            identity: ServerIdentity {
                data: PathBuf::from("/probe/data"),
                config: PathBuf::from("/probe/config"),
            },
            epoch: epoch.to_string(),
            process: ProcessProvenance {
                pid,
                created: "process-created".to_string(),
                executable: PathBuf::from("/probe/domios-server"),
            },
            api_address: "127.0.0.1:7860".parse::<SocketAddr>().unwrap(),
            socket_address: "probe.sock".to_string(),
            bootstrap_address: "bootstrap.sock".to_string(),
            resources: PathBuf::from("/probe/resources"),
        }
    }

    fn reply(expected: &ServerReady, challenge: &str) -> AttachReply {
        AttachReply {
            challenge: challenge.to_string(),
            ready: expected.clone(),
            api_token: None,
        }
    }

    fn shutdown_request(expected: &ServerReady, challenge: &str) -> AttachRequest {
        AttachRequest {
            protocol: expected.protocol,
            api_version: expected.api_version,
            build: expected.build.clone(),
            identity: expected.identity.clone(),
            challenge: challenge.to_string(),
            operation: BootstrapOperation::Shutdown,
            expected: Some(ServerExpectation {
                process: expected.process.clone(),
                epoch: expected.epoch.clone(),
            }),
        }
    }

    #[test]
    fn lifecycle_actions_keep_window_close_and_crash_separate_from_server_shutdown() {
        assert_eq!(
            plan_client_lifecycle(ClientLifecycleEvent::ServerUnavailable),
            ClientLifecycleAction::ShowReconnectingAndPreserveUi
        );
        assert_eq!(
            plan_client_lifecycle(ClientLifecycleEvent::MainWindowClose),
            ClientLifecycleAction::CloseWindowKeepOwners
        );
        assert_eq!(
            plan_client_lifecycle(ClientLifecycleEvent::ClientCrashRelaunch),
            ClientLifecycleAction::ReconnectToExistingServer
        );
        assert_eq!(
            plan_client_lifecycle(ClientLifecycleEvent::ExplicitQuit),
            ClientLifecycleAction::ShutdownExactServerThenExitClient
        );
        assert_eq!(
            plan_client_lifecycle(ClientLifecycleEvent::AuthorizedServerOrPairUpdate),
            ClientLifecycleAction::ShutdownExactServerThenExitClient
        );
        assert_eq!(
            plan_client_lifecycle(ClientLifecycleEvent::ClientOnlyUpdate),
            ClientLifecycleAction::ExitClientOnly
        );
        assert_eq!(
            plan_client_lifecycle(ClientLifecycleEvent::UnixSignal),
            ClientLifecycleAction::ExitClientOnly
        );
    }

    #[test]
    fn exit_gate_coalesces_requests_and_only_releases_proven_predispatch_refusal() {
        let gate = ClientExitGate::default();
        let ticket = gate.begin(ClientExitReason::ExplicitQuit).unwrap();
        assert_eq!(
            gate.begin(ClientExitReason::ExplicitQuit),
            Err(ClientExitRefusal::AlreadyInProgress)
        );
        assert!(gate.refuse_before_dispatch(ticket));

        let next = gate.begin(ClientExitReason::ExplicitQuit).unwrap();
        assert!(!gate.refuse_before_dispatch(ticket));
        assert!(gate.mark_uncertain(next));
        assert!(!gate.refuse_before_dispatch(next));
        assert!(gate.status().is_some_and(|status| {
            status.operation_id == next.operation_id && status.phase == ClientExitPhase::Uncertain
        }));
        assert!(gate.confirm(next));
        assert!(!gate.mark_uncertain(next));
        assert!(gate.status().is_some_and(|status| {
            status.operation_id == next.operation_id && status.phase == ClientExitPhase::Confirmed
        }));
    }

    #[test]
    fn shutdown_reply_requires_the_exact_server_snapshot_and_challenge() {
        let expected = ready("epoch-old", 41);
        assert!(is_exact_server_shutdown_reply(
            &expected,
            &shutdown_request(&expected, "challenge-1"),
            &reply(&expected, "challenge-1")
        ));
        assert!(!is_exact_server_shutdown_reply(
            &expected,
            &shutdown_request(&expected, "challenge-1"),
            &reply(&ready("epoch-new", 41), "challenge-1")
        ));
        assert!(!is_exact_server_shutdown_reply(
            &expected,
            &shutdown_request(&expected, "challenge-1"),
            &reply(&ready("epoch-old", 42), "challenge-1")
        ));
        let mut wrong_operation = shutdown_request(&expected, "challenge-1");
        wrong_operation.operation = BootstrapOperation::Inspect;
        assert!(!is_exact_server_shutdown_reply(
            &expected,
            &wrong_operation,
            &reply(&expected, "challenge-1")
        ));
        let mut wrong_expected = shutdown_request(&expected, "challenge-1");
        wrong_expected.expected = Some(ServerExpectation {
            process: expected.process.clone(),
            epoch: "epoch-new".to_string(),
        });
        assert!(!is_exact_server_shutdown_reply(
            &expected,
            &wrong_expected,
            &reply(&expected, "challenge-1")
        ));
        assert!(!is_exact_server_shutdown_reply(
            &expected,
            &shutdown_request(&expected, "challenge-1"),
            &reply(&expected, "challenge-stale")
        ));
    }

    #[test]
    fn shutdown_reply_requires_the_current_shared_bootstrap_protocol() {
        let expected = ready("epoch-old", 41);
        let previous_protocol = BOOTSTRAP_PROTOCOL.saturating_sub(1);
        let mut previous_ready = expected.clone();
        previous_ready.protocol = previous_protocol;
        let previous_request = shutdown_request(&previous_ready, "challenge-1");
        assert!(!is_exact_server_shutdown_reply(
            &previous_ready,
            &previous_request,
            &reply(&previous_ready, "challenge-1")
        ));

        let mut previous_request = shutdown_request(&expected, "challenge-1");
        previous_request.protocol = previous_protocol;
        assert!(!is_exact_server_shutdown_reply(
            &expected,
            &previous_request,
            &reply(&expected, "challenge-1")
        ));
    }
}
