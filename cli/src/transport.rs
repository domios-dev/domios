//! `transport_for` has no wildcard arm, so a new `Commands` variant fails to compile.
//! The two mesh fields disagree today; `SocketRpc` returns before `ApiVerdict` is read.

pub(crate) mod pre_socket;

use crate::{Commands, MemAction, TeamAction};

const TEAM_LOCAL_ONLY: &str = "team management is local-only — run it on the target machine";
const TASK_LOCAL_ONLY: &str = "task management is local-only — run it on the target machine";

/// Where a command is handled before the socket dispatch match sees it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Local {
    /// Handled directly after the `--mesh` branch and before socket discovery.
    PreMesh,
    /// Handled after the `--mesh` branch but before socket discovery.
    PreSocket,
    /// Offered to a local handler that may decline and fall through to the socket.
    PreSocketOrSocket,
    /// Reaches `SocketClient::connect` and the dispatch match.
    Socket,
}

/// Layer one: what `run_via_mesh` does before consulting `api_map`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MeshRelay {
    /// Relayed as a socket RPC through `MeshRpcClient`; `api_map` is never reached.
    SocketRpc,
    /// Continues into the REST mapping.
    Fallthrough,
}

/// Layer two: `api_map`'s own verdict, recorded even where layer one makes it
/// unreachable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ApiVerdict {
    /// `api_map` builds a REST method, path, and body for it.
    Rest,
    /// Refused with this exact reason, carried verbatim from the old arms.
    Unsupported(&'static str),
    /// Refuses with its own message and exit code instead of a verdict.
    RefusedInPlace,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Transport {
    pub local: Local,
    pub mesh_relay: MeshRelay,
    pub api_verdict: ApiVerdict,
}

const fn socket_rest() -> Transport {
    Transport {
        local: Local::Socket,
        mesh_relay: MeshRelay::Fallthrough,
        api_verdict: ApiVerdict::Rest,
    }
}

const fn socket_unsupported(reason: &'static str) -> Transport {
    Transport {
        local: Local::Socket,
        mesh_relay: MeshRelay::Fallthrough,
        api_verdict: ApiVerdict::Unsupported(reason),
    }
}

/// Relayed as a socket RPC, so the recorded verdict is unreachable today.
const fn socket_rpc_relayed(unreachable_reason: &'static str) -> Transport {
    Transport {
        local: Local::Socket,
        mesh_relay: MeshRelay::SocketRpc,
        api_verdict: ApiVerdict::Unsupported(unreachable_reason),
    }
}

const fn pre_socket_unsupported(reason: &'static str) -> Transport {
    Transport {
        local: Local::PreSocket,
        mesh_relay: MeshRelay::Fallthrough,
        api_verdict: ApiVerdict::Unsupported(reason),
    }
}

pub(crate) fn transport_for(command: &Commands) -> Transport {
    match command {
        Commands::Identify => socket_rest(),
        Commands::Capabilities => {
            socket_unsupported("capabilities is socket-only; run it on the target machine")
        }
        Commands::Pad { .. } => {
            socket_unsupported("pad is caller-local and cannot target a mesh peer")
        }
        Commands::Plugin { .. } => Transport {
            local: Local::PreMesh,
            mesh_relay: MeshRelay::Fallthrough,
            api_verdict: ApiVerdict::Unsupported(
                "plugin management is local-only — run it on the target machine",
            ),
        },
        Commands::CliManifest { .. } => pre_socket_unsupported(
            "cli-manifest is local-only — it dumps this binary's own command tree",
        ),
        Commands::Workspace { .. } => socket_rest(),
        Commands::Team { action } => team_transport(action),
        Commands::Orchestrator { .. } => {
            socket_unsupported("orchestrator handoff is local-only — run it on the target machine")
        }
        Commands::Persona { .. } => socket_unsupported(
            "persona inspection is socket-only and answers for that daemon's own overlay directory — run it on the target machine",
        ),
        Commands::Tab { .. } => socket_rest(),
        Commands::SendText { .. } => socket_rest(),
        Commands::Delayed { .. } => socket_rest(),
        Commands::Web { .. } => socket_unsupported(
            "web link is local-only — each peer's link needs that peer's own token; run it there",
        ),
        Commands::Paste { .. } => socket_rest(),
        Commands::Notify { .. } => socket_rest(),
        Commands::Keepawake { .. } => socket_rest(),
        Commands::Theme { .. } => socket_rest(),
        Commands::Agent { .. } => socket_rest(),
        Commands::Cron { .. } => socket_rest(),
        Commands::Service { .. } => socket_rest(),
        Commands::Update { .. } => socket_rest(),
        Commands::Config { .. } => Transport {
            local: Local::PreSocket,
            mesh_relay: MeshRelay::Fallthrough,
            api_verdict: ApiVerdict::Rest,
        },
        Commands::Bridge { .. } => socket_rest(),
        Commands::Mesh { .. } => socket_rest(),
        Commands::Mem { action } => Transport {
            local: mem_local(action),
            mesh_relay: MeshRelay::Fallthrough,
            api_verdict: ApiVerdict::Rest,
        },
        Commands::Plan { .. } => socket_rpc_relayed(
            "plan management is local-only — run it on the target machine",
        ),
        Commands::Spec { .. } => socket_rpc_relayed(
            "spec management is local-only — run it on the target machine",
        ),
        Commands::Effort { .. } => socket_rpc_relayed(
            "effort management is local-only — run it on the target machine",
        ),
        Commands::Task { .. } => socket_rpc_relayed(TASK_LOCAL_ONLY),
        Commands::Inbox { .. } => socket_rpc_relayed(TASK_LOCAL_ONLY),
        // Canvas is not in the relayed group, so its verdict is reachable.
        Commands::Canvas { .. } => {
            socket_unsupported("canvas control is local-only — run it on the target machine")
        }
        Commands::Sessions { .. } => socket_rest(),
        Commands::Usage { .. } => socket_rest(),
        Commands::System { .. } => pre_socket_unsupported(
            "system configuration is local-only — run it on the target machine",
        ),
        Commands::Docs { .. } => pre_socket_unsupported(
            "docs are embedded in this binary — run the command on the target machine",
        ),
        Commands::Debug { .. } => Transport {
            local: Local::Socket,
            mesh_relay: MeshRelay::Fallthrough,
            api_verdict: ApiVerdict::RefusedInPlace,
        },
    }
}

/// `team pair` is the one Team action that reaches a peer over REST; the other
/// six relay as socket RPCs, which makes their recorded verdict unreachable.
fn team_transport(action: &TeamAction) -> Transport {
    match action {
        TeamAction::Pair { .. } => socket_rest(),
        TeamAction::List
        | TeamAction::Show { .. }
        | TeamAction::Rename { .. }
        | TeamAction::Close { .. }
        | TeamAction::AdoptPlan { .. }
        | TeamAction::AdoptAgent { .. } => socket_rpc_relayed(TEAM_LOCAL_ONLY),
    }
}

/// `mem` splits below the variant: three sub-paths open knowledge.db directly.
/// `graph` may decline and continue to the socket; `dream` never returns.
fn mem_local(action: &MemAction) -> Local {
    if matches!(
        action,
        MemAction::RunWorker { .. } | MemAction::Dream { .. }
    ) {
        Local::PreSocket
    } else if matches!(action, MemAction::Graph { .. }) {
        Local::PreSocketOrSocket
    } else {
        Local::Socket
    }
}

pub(crate) fn is_socket_relayed(command: &Commands) -> bool {
    transport_for(command).mesh_relay == MeshRelay::SocketRpc
}

pub(crate) fn should_dispatch_direct_local(command: &Commands, mesh_target: Option<&str>) -> bool {
    mesh_target.is_none() && transport_for(command).local == Local::PreMesh
}
