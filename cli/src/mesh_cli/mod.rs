//! Mesh CLI dispatch — relay local commands to a paired peer via /api/mesh/relay.
//!
//! Submodules:
//! - `auth`     — Ed25519 signing for outbound peer requests
//! - `update`   — two-step `update --apply` over mesh (force-check then install)
//! - `relay`    — generic relay envelope + peer URL resolution
//! - `api_map`  — translate any local CLI command to its API method+path+body
//! - `parse`    — small input parsers (ip[:port])

use clap::Subcommand;

use super::{Commands, UpdateAction};

mod api_map;
mod auth;
mod parse;
mod relay;
mod update;

#[cfg(test)]
mod tests;

pub use auth::sign_request;
pub use parse::parse_ip_port;
pub use relay::mesh_relay_cmd;

#[derive(Subcommand)]
pub enum MeshAction {
    /// Show mesh status (identity, discovery, peers)
    Status,
    /// List all peers (paired + discovered)
    Peers,
    /// Execute a command on a remote mesh peer
    Exec {
        /// Target peer name or instance ID
        target: String,
        /// Command to execute
        command: String,
        /// Target pane on remote instance (default: general)
        #[arg(long, default_value = "general")]
        pane: String,
    },
    /// Send text to a pane on a remote mesh peer
    Send {
        /// Target peer name or instance ID
        target: String,
        /// Text to send
        text: String,
        /// Target pane on remote instance (default: general)
        #[arg(long, default_value = "general")]
        pane: String,
    },
    /// Get output from a pane on a remote mesh peer
    Output {
        /// Target peer name or instance ID
        target: String,
        /// Target pane on remote instance (default: general)
        #[arg(long, default_value = "general")]
        pane: String,
    },
    /// List panes on a remote mesh peer
    Panes {
        /// Target peer name or instance ID
        target: String,
    },
    /// Initiate pairing with a discovered peer
    Connect {
        /// Instance ID of the discovered peer
        instance_id: String,
    },
    /// Accept an incoming pairing request
    Approve {
        /// Request ID of the pending pairing request
        request_id: String,
    },
    /// Reject and block an incoming pairing request
    Reject {
        /// Request ID of the pending pairing request
        request_id: String,
    },
    /// Disconnect (unpair) from a paired peer
    Disconnect {
        /// Instance ID of the paired peer
        instance_id: String,
    },
    /// Trigger mesh discovery scan (list discovered peers)
    Discover,
    /// Start broadcasting for the configured duration
    Broadcast,
    /// Stop broadcasting immediately
    #[clap(name = "stop-broadcast")]
    StopBroadcast,
    /// Re-detect local addresses (Tailscale + LAN) and announce them to all
    /// paired peers. Use after a network swap so peers update their cache
    /// without restarting CodeTerm.
    Refresh,
    /// Push our current address list to paired peers without re-detecting
    /// locally. Use --to to limit to one peer.
    Announce {
        /// Optional target peer (instance_id or name). Omit for all peers.
        #[arg(long)]
        to: Option<String>,
    },
    /// Per-peer address management.
    Peer {
        #[command(subcommand)]
        action: MeshPeerAction,
    },
}

#[derive(Subcommand)]
pub enum MeshPeerAction {
    /// Manually override our cached address for a paired peer. Useful when
    /// auto-detection is wrong or you want to point at a specific interface.
    /// Format: ip[:port]. Examples:
    ///   codeterm mesh peer set-addr BLACKSTAR 100.84.159.37
    ///   codeterm mesh peer set-addr BLACKSTAR 192.168.1.42:7685
    #[clap(name = "set-addr")]
    SetAddr {
        /// Target peer (instance_id or name)
        peer: String,
        /// New address as ip[:port]
        addr: String,
    },
}

/// Execute a CLI command on a remote mesh peer via relay.
/// Reconstructs the command as an API call and sends through relay proxy.
pub fn run_via_mesh(cli: &super::Cli, target: &str) {
    // Mesh management commands are local-only — cannot be relayed.
    if let super::Commands::Mesh { action } = &cli.command {
        match action {
            MeshAction::Connect { .. } | MeshAction::Approve { .. } |
            MeshAction::Reject { .. } | MeshAction::Disconnect { .. } |
            MeshAction::Discover | MeshAction::Broadcast | MeshAction::StopBroadcast |
            MeshAction::Peer { .. } => {
                eprintln!("Error: mesh management commands are local-only and cannot be relayed via --mesh");
                std::process::exit(1);
            }
            _ => {} // Status, Peers, Exec, Send, Output, Panes, Refresh, Announce — relayable
        }
    }

    let (base, token) = super::read_api_config();
    let http = super::build_http(&token);

    // `update --apply` over mesh is a two-step (force-check then install) flow,
    // not a single relay call.
    if let Commands::Update { action, check, apply, channel, manifest_url, after_update } = &cli.command {
        let resolved = update::resolve_update_action(action.as_ref(), *check, *apply, channel.clone(), manifest_url.clone(), after_update.clone());
        if let UpdateAction::Install { auto: true, manifest_url, from_file: _, continue_prompt: _, continue_pane: _, after_update } = &resolved {
            update::run_update_apply_via_mesh(&http, &base, &token, target, manifest_url.as_deref(), after_update.as_deref(), cli.json);
            return;
        }
    }

    let (method, path, body) = api_map::cli_command_to_api(&cli.command);
    relay::mesh_relay_generic(&http, &base, &token, target, &method, &path, body.as_deref(), cli.json);
}
