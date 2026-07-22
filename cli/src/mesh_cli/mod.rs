//! Mesh CLI dispatch: relay local commands to paired peers via /api/mesh/relay.
//! Submodules: auth, update, relay, api_map, parse.

use clap::Subcommand;

use super::{Commands, UpdateAction};

mod api_map;
mod auth;
mod parse;
mod relay;
mod rpc;
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
    ///   codeterm mesh peer set-addr BLACKSTAR 100.64.10.1
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
pub fn run_via_mesh(cli: super::Cli, target: &str) {
    if matches!(
        &cli.command,
        Commands::Plan { .. }
            | Commands::Team { .. }
            | Commands::Spec { .. }
            | Commands::Effort { .. }
            | Commands::Task { .. }
    ) {
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

    let (base, token) = super::read_api_config();
    let http = match super::build_http(&token) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Error: failed to build HTTP client: {}", e);
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
            after_update,
        } = &resolved
        {
            update::run_update_apply_via_mesh(
                &http,
                &base,
                &token,
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
        action: super::PaneAction::Search { pattern },
    } = &cli.command
    {
        let panes =
            relay::mesh_relay_fetch_json(&http, &base, &token, target, "GET", "/api/panes", None);
        let arr = panes.as_array().cloned().unwrap_or_default();
        crate::cmd::pane::print_pane_find_results(cli.json, pattern, &arr);
        return;
    }

    // `tab list` reuses local formatter: relay raw list, then format locally.
    if !cli.json {
        if let Commands::Tab {
            action: super::PaneAction::List { agents },
        } = &cli.command
        {
            let panes = relay::mesh_relay_fetch_json(
                &http,
                &base,
                &token,
                target,
                "GET",
                "/api/panes",
                None,
            );
            let mut arr = panes.as_array().cloned().unwrap_or_default();
            if *agents {
                arr.retain(|p| {
                    p.get("agent_type")
                        .and_then(|v| v.as_str())
                        .is_some_and(|s| !s.is_empty())
                });
            }
            if arr.is_empty() {
                println!("(no panes)");
            } else {
                print!("{}", crate::cmd::pane::render_pane_tree(&arr));
            }
            return;
        }
    }

    let (method, path, body) = api_map::cli_command_to_api(&cli.command);
    if method == api_map::UNSUPPORTED_OVER_MESH {
        eprintln!("Error: {}", path);
        std::process::exit(2);
    }
    // sidebar ghosts it under its report target immediately (before any report).
    if matches!(
        cli.command,
        Commands::Agent {
            action: super::AgentAction::Spawn { .. }
        }
    ) {
        relay::mesh_spawn_and_register(&http, &base, &token, target, body.as_deref(), cli.json);
        return;
    }
    if let Commands::Tab {
        action: super::PaneAction::Close { id },
    } = &cli.command
    {
        relay::mesh_close_pane_and_forget_remote_worker(&http, &base, &token, target, id, cli.json);
        return;
    }
    let body = if matches!(cli.command, Commands::SendText { .. }) {
        body.map(|b| enrich_send_body_with_local_source(&http, &base, &token, b))
    } else {
        body
    };
    relay::mesh_relay_generic(
        &http,
        &base,
        &token,
        target,
        &method,
        &path,
        body.as_deref(),
        cli.json,
    );
}

/// Look up the local source pane's display label + agent model and inject them
/// into a `send` body as `source_label` / `source_model`. The peer applies the
/// canonical CtxPrefix with these (its own mux can't resolve a foreign pane id).
fn enrich_send_body_with_local_source(
    http: &reqwest::blocking::Client,
    base: &str,
    token: &Option<String>,
    body: String,
) -> String {
    let Ok(mut v) = serde_json::from_str::<serde_json::Value>(&body) else {
        return body;
    };
    let Some(src) = v
        .get("source_pane")
        .and_then(|x| x.as_str())
        .map(str::to_string)
    else {
        return body;
    };
    if src.is_empty() || src == "unknown" {
        return body;
    }
    let panes = http
        .get(format!("{}/api/panes", base))
        .header(
            "Authorization",
            format!("Bearer {}", token.as_deref().unwrap_or("")),
        )
        .send()
        .ok()
        .and_then(|r| r.json::<serde_json::Value>().ok());
    let Some(arr) = panes.as_ref().and_then(|p| p.as_array()) else {
        return body;
    };
    let Some(pane) = arr.iter().find(|p| {
        p.get("id")
            .and_then(|x| x.as_str())
            .is_some_and(|id| id == src || id.starts_with(&src))
    }) else {
        return body;
    };
    let title = pane
        .get("title")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    let tab_title = pane
        .get("tab_title")
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .trim();
    // Mirror display_label_for_pane: tab title wins only when it differs.
    let label = if !tab_title.is_empty() && tab_title != title {
        tab_title
    } else {
        title
    };
    if !label.is_empty() {
        v["source_label"] = serde_json::json!(label);
    }
    if let Some(model) = pane
        .get("agent_session_model")
        .and_then(|x| x.as_str())
        .filter(|m| !m.is_empty())
    {
        v["source_model"] = serde_json::json!(model);
    }
    v.to_string()
}
