//! Mesh CLI dispatch: relay local commands to paired peers via /api/mesh/relay.
//! Submodules: auth, update, relay, api_map, parse.

use clap::Subcommand;

use super::{Commands, TabAction, UpdateAction};

pub(crate) mod api_map;
#[cfg(test)]
mod compat;
mod parse;
mod relay;
mod rpc;
mod tab_list;
mod tty_line_guard;
mod update;
mod workspace_topology;

#[cfg(test)]
mod tests;

pub use parse::parse_ip_port;
pub use relay::mesh_relay_cmd;
pub(crate) use relay::{mesh_relay_fetch_json, mesh_relay_invoke_result};

fn remote_close_tab_id(command: &Commands) -> Option<&str> {
    match command {
        Commands::Tab {
            action:
                super::TabAction::Close {
                    id: Some(id),
                    tab: None,
                },
        } => Some(id),
        _ => None,
    }
}

#[derive(Subcommand)]
pub enum MeshAction {
    /// Show mesh status (identity, discovery, peers)
    Status,
    /// List all peers (paired + discovered)
    Peers,
    /// List tabs on a remote mesh peer
    Tabs {
        /// Target peer name or instance ID
        target: String,
    },
    /// Initiate pairing with a discovered peer
    Connect {
        /// Instance ID or name of the discovered peer
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
    /// Explicitly lift a peer identity revocation so it can pair again
    Unrevoke {
        /// Instance ID of the revoked peer
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
    /// Go offline: stop pinging peers, stop announcing, stop refreshing
    /// local addresses (which keeps Tailscale alive). The daemon stays up;
    /// peers will time out our presence on their side. Local-only command.
    Offline,
    /// Come back online — resume mesh activity. Local-only.
    Online,
}

#[derive(Subcommand)]
pub enum MeshPeerAction {
    /// Manually override our cached address for a paired peer. Useful when
    /// auto-detection is wrong or you want to point at a specific interface.
    /// Format: ip[:port]. Examples:
    ///   codeterm mesh peer set-addr PEER-NAME 100.100.100.100
    ///   codeterm mesh peer set-addr PEER-NAME 192.168.1.42:7685
    #[clap(name = "set-addr")]
    SetAddr {
        /// Target peer (instance_id or name)
        peer: String,
        /// New address as ip[:port]
        addr: String,
    },
}

pub(crate) fn is_socket_relayed(command: &Commands) -> bool {
    crate::transport::is_socket_relayed(command)
}

pub(crate) fn run_plan_task_dispatch_origin(cli: super::Cli, peer: &str) {
    let Commands::Plan {
        action:
            crate::PlanAction::Task {
                action: task @ crate::args::plan::PlanTaskAction::Dispatch { .. },
            },
    } = cli.command
    else {
        return;
    };
    let (mut client, _) = match crate::client::SocketClient::connect_local() {
        Ok(connection) => connection,
        Err(error) => {
            eprintln!("Error: {error}");
            std::process::exit(1);
        }
    };
    crate::cmd::plan::handle_mesh_dispatch(&mut client, cli.json, task, peer);
}

/// Execute a CLI command on a remote mesh peer via relay.
/// Reconstructs the command as an API call and sends through relay proxy.
pub fn run_via_mesh(cli: super::Cli, target: &str) {
    if is_socket_relayed(&cli.command) {
        let (base, token) = super::read_api_config();
        let mut client = match rpc::MeshRpcClient::connect(&base, &token, target) {
            Ok(client) => client,
            Err(error) => {
                eprintln!("Error: {error}");
                std::process::exit(1);
            }
        };
        match cli.command {
            Commands::Plan { action } => {
                crate::cmd::plan::handle_plan(&mut client, cli.json, action)
            }
            Commands::Team { action } => crate::cmd::team::handle(&mut client, cli.json, action),
            Commands::Spec { action } => {
                crate::cmd::spec::handle_spec(&mut client, cli.json, action)
            }
            Commands::Effort { action } => {
                crate::cmd::effort::handle_effort(&mut client, cli.json, action)
            }
            Commands::Task { action } => {
                crate::cmd::task::handle_task(&mut client, cli.json, action)
            }
            Commands::Inbox { action } => {
                crate::cmd::task::handle_inbox(&mut client, cli.json, action)
            }
            _ => unreachable!(),
        }
        return;
    }

    // Mesh management commands are local-only — cannot be relayed.
    if let super::Commands::Mesh {
        action:
            MeshAction::Connect { .. }
            | MeshAction::Approve { .. }
            | MeshAction::Reject { .. }
            | MeshAction::Disconnect { .. }
            | MeshAction::Unrevoke { .. }
            | MeshAction::Discover
            | MeshAction::Broadcast
            | MeshAction::StopBroadcast
            | MeshAction::Peer { .. }
            | MeshAction::Offline
            | MeshAction::Online,
    } = &cli.command
    {
        eprintln!(
            "Error: mesh management commands are local-only and cannot be relayed via --mesh"
        );
        std::process::exit(1);
    }

    let (base, _) = super::read_api_config();
    let local = match super::build_local_api() {
        Ok(client) => client,
        Err(e) => {
            eprintln!("Error: failed to connect to local API: {}", e);
            std::process::exit(1);
        }
    };

    // `update install --auto` is a two-step flow: force-check then install.
    if let Commands::Update {
        action,
        manifest_url,
        after_update,
    } = &cli.command
    {
        let resolved = update::resolve_update_action(
            action.as_ref(),
            manifest_url.clone(),
            after_update.clone(),
        );
        if let UpdateAction::Install {
            auto: true,
            manifest_url,
            from_file: None,
            continue_prompt: None,
            continue_tab: None,
            no_continue: false,
            after_update,
        } = &resolved
        {
            update::run_update_apply_via_mesh(
                target,
                manifest_url.as_deref(),
                after_update.as_deref(),
                cli.json,
            );
            return;
        }
    }

    // tab search needs local fuzzy ranking; relay-only output has no relevance ordering.
    // Fetch raw list then rank locally for predictable results.
    if let Commands::Tab {
        action: super::TabAction::Search { pattern },
    } = &cli.command
    {
        let tabs = relay::mesh_relay_fetch_json(target, "GET", "/api/tabs", None);
        let arr = tabs.as_array().cloned().unwrap_or_default();
        crate::cmd::tab::print_tab_find_results(cli.json, pattern, &arr);
        return;
    }

    // Human list views reuse local formatters over remote state.
    if workspace_topology::should_render(&cli.command, cli.json) {
        workspace_topology::print_remote_workspace_topology(target);
        return;
    }
    // Tab list is formatted locally in both modes, so `--json` emits the same
    // envelope as the local path instead of the peer's bare REST array.
    if let Some(rendering) = tab_list::rendering_for(&cli) {
        let (_, path, _) = api_map::cli_command_to_api(&cli.command);
        let answer = relay::mesh_relay_invoke_with_peer(target, "GET", &path, None, cli.json);
        if !cli.json {
            relay::announce_answering_peer(target, answer.peer.as_ref());
        }
        let tabs = answer.data;
        let requested_workspace = match &cli.command {
            Commands::Tab {
                action: TabAction::List { workspace, .. },
            } => workspace.as_deref(),
            _ => None,
        };
        tab_list::print(rendering, &tabs, requested_workspace, answer.peer.as_ref());
        return;
    }

    let (method, path, body) = api_map::cli_command_to_api(&cli.command);
    if method == api_map::UNSUPPORTED_OVER_MESH {
        eprintln!("Error: {}", path);
        std::process::exit(2);
    }
    if !cli.json
        && matches!(
            cli.command,
            Commands::Mem {
                action: super::MemAction::Search { .. }
            }
        )
    {
        let answer = relay::mesh_relay_invoke_with_peer(target, &method, &path, None, false);
        relay::announce_answering_peer(target, answer.peer.as_ref());
        print!("{}", crate::cmd::mem::render_search_hits(&answer.data));
        return;
    }
    // sidebar ghosts it under its report target immediately (before any report).
    if matches!(
        cli.command,
        Commands::Agent {
            action: super::AgentAction::Spawn { .. }
        }
    ) {
        relay::mesh_spawn_and_register(target, body.as_deref(), cli.json);
        return;
    }
    if let Some(id) = remote_close_tab_id(&cli.command) {
        relay::mesh_close_tab_and_forget_remote_worker(&base, target, id, cli.json);
        return;
    }
    if matches!(cli.command, Commands::SendText { to_shell: true, .. }) {
        tty_line_guard::enforce(target, body.as_deref(), cli.json);
    }
    let body = if matches!(cli.command, Commands::SendText { .. }) {
        body.map(|b| enrich_send_body_with_local_source(&local, &base, b))
    } else {
        body
    };
    relay::mesh_relay_generic(target, &method, &path, body.as_deref(), cli.json);
}

/// Look up the local source tab's display label + agent model and inject them
/// into a `send` body as `source_label` / `source_model`. The peer applies the
/// canonical CtxPrefix with these (its own mux can't resolve a foreign tab id).
fn enrich_send_body_with_local_source(
    local: &crate::local_api::LocalApiClient,
    base: &str,
    body: String,
) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return body;
    };
    let Some(src) = v
        .get("source_tab")
        .and_then(|x| x.as_str())
        .map(str::to_string)
    else {
        return body;
    };
    if src.is_empty() || src == "unknown" {
        return body;
    }
    let tabs = local
        .get(format!("{}/api/tabs", base))
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok());
    let Some(arr) = tabs.as_ref().and_then(|p| p.as_array()) else {
        return body;
    };
    let Some(tab) = arr.iter().find(|p| {
        p.get("id")
            .and_then(|x| x.as_str())
            .is_some_and(|id| id == src || id.starts_with(&src))
    }) else {
        return body;
    };
    let label = crate::tab_name::tab_name(tab);
    if !label.is_empty() {
        v["source_label"] = serde_json::json!(label);
    }
    if let Some(model) = tab
        .get("agent_session_model")
        .and_then(|x| x.as_str())
        .filter(|m| !m.is_empty())
    {
        v["source_model"] = serde_json::json!(model);
    }
    v["source_is_agent"] = serde_json::json!(
        tab.get("agent_type")
            .and_then(|value| value.as_str())
            .is_some()
            || tab
                .get("agent_booting")
                .and_then(|value| value.as_bool())
                .unwrap_or(false)
    );
    v.to_string()
}
