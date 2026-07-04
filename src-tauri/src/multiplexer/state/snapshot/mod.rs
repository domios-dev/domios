use std::collections::HashMap;

use super::{MultiplexerSnapshot, MultiplexerState};
use crate::multiplexer::types::{Pane, PaneBacking, PaneType, Tab, TitleLevel, Workspace};

fn derive_agent_type_from_title(title: Option<&str>) -> Option<String> {
    let t = title?.to_ascii_lowercase();
    for id in ["claude", "codex", "opencode", "cursor"] {
        if t.contains(id) {
            return Some(id.to_string());
        }
    }
    None
}

fn should_seed_from_legacy_tab_title(
    pane_title: &str,
    legacy_tab_title: &str,
    has_agent_type: bool,
) -> bool {
    if legacy_tab_title.is_empty() {
        return false;
    }
    let pane_title = pane_title.trim();
    pane_title.is_empty()
        || pane_title == "Terminal"
        || (has_agent_type && derive_agent_type_from_title(Some(pane_title)).is_some())
}

impl MultiplexerState {
    /// Return all data for frontend hydration. Each call stamps a strictly
    /// increasing `generation`; the stamp happens while the caller holds the
    /// (read) lock, so it is serialized against writers and a snapshot with
    /// newer content always carries a higher generation than an older one.
    pub fn snapshot(&self) -> MultiplexerSnapshot {
        let generation = self
            .generation
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst)
            .wrapping_add(1);
        MultiplexerSnapshot {
            workspaces: self.workspaces.clone(),
            panes: self.panes.clone(),
            active_workspace_id: self.active_workspace_id.clone(),
            focused_pane_id: self.focused_pane_id.clone(),
            generation,
            // Bounded indexed query against the local sqlite store — cheap, not
            // the multi-second blocking work the hot-path rule warns about, and
            recently_closed: self.recent_closed_view(),
            pane_statuses: HashMap::new(),
        }
    }

    /// Same as `snapshot()`, plus each pane's live aggregate status — the
    /// snapshot path frontend hydration (`mux_get_state` / `mux_state`) needs
    /// so status survives a page reload instead of staying blank until the
    /// next `pane-status-changed` event.
    ///
    /// A pane with an active `pending_prompt` hydrates as `Waiting` even
    /// though the PTY-output tracker (which only ever sees Working/Idle from
    /// screen-hash churn) never observed it: the live `prompt-state-changed`
    /// path sets `pending_prompt` directly on the pane and never touches
    /// `PaneStatusTracker`, so without this override a reload would blank the
    /// attention rail back to whatever the raw output tracker last saw. Takes
    /// priority over everything except a PTY the tracker confirmed `Dead` —
    /// a dead pane can't meaningfully be "waiting".
    pub fn snapshot_with_status(
        &self,
        agent_tracker: &crate::agent::status::PaneStatusTracker,
    ) -> MultiplexerSnapshot {
        use crate::agent::status::PaneStatus;

        let mut snap = self.snapshot();
        snap.pane_statuses = self
            .panes
            .iter()
            .filter_map(|(pane_id, pane)| {
                let tracked = pane
                    .pty_id
                    .as_deref()
                    .and_then(|pty_id| agent_tracker.get_status_if_tracked(pty_id));
                if pane.pending_prompt.is_some() && !matches!(tracked, Some(PaneStatus::Dead)) {
                    return Some((pane_id.clone(), PaneStatus::Waiting));
                }
                tracked.map(|status| (pane_id.clone(), status))
            })
            .collect();
        snap
    }

    /// Create a session snapshot for persistence.
    pub fn to_session_snapshot(&self) -> crate::session::store::SessionSnapshot {
        use crate::session::store::*;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn pane_to_leaf_snapshot(
            pane_id: &str,
            panes: &HashMap<String, Pane>,
        ) -> SplitNodeSnapshot {
            let pane = panes.get(pane_id);
            let (chatbackend_plugin_id, chatbackend_session_id) = match pane.map(|p| &p.backing) {
                Some(PaneBacking::ChatBackend {
                    plugin_id,
                    session_id,
                }) => (Some(plugin_id.clone()), Some(session_id.clone())),
                _ => (None, None),
            };
            SplitNodeSnapshot::Leaf {
                pane_id: pane_id.to_string(),
                cwd: pane.and_then(|p| p.cwd.clone()),
                title: pane.map(|p| p.title.clone()),
                description: pane.and_then(|p| p.description.clone()),
                title_level: pane.map(|p| format!("{:?}", p.title_level)),
                agent_type: pane.and_then(|p| p.agent_type.clone()),
                agent_type_locked: pane.map(|p| p.agent_type_locked).unwrap_or(false),
                claude_session_id: pane.and_then(|p| p.agent_session_id.clone()),
                role: pane.and_then(|p| p.role.map(|r| format!("{:?}", r))),
                role_profile: pane.and_then(|p| p.role_profile.clone()),
                parent_pane_id: pane.and_then(|p| p.parent_pane_id.clone()),
                spawned_by_pane_id: pane.and_then(|p| p.spawned_by_pane_id.clone()),
                report_target_pane_id: pane.and_then(|p| p.report_target_pane_id.clone()),
                report_target_mesh_peer: pane.and_then(|p| p.report_target_mesh_peer.clone()),
                can_spawn: pane.map(|p| p.can_spawn).unwrap_or(false),
                depth: pane.map(|p| p.depth).unwrap_or(0),
                chat_view: pane.map(|p| p.chat_view),
                auto_approve: pane.map(|p| p.auto_approve).unwrap_or(false),
                agent_session_model: pane.and_then(|p| p.agent_session_model.clone()),
                chatbackend_plugin_id,
                chatbackend_session_id,
            }
        }

        let workspaces = self
            .workspaces
            .iter()
            .map(|ws| {
                WorkspaceSnapshot {
                    id: ws.id.clone(),
                    name: ws.name.clone(),
                    active_tab_id: ws.active_tab_id.clone(),
                    cwd: ws.cwd.clone(),
                    protected: ws.protected,
                    sort_order: ws.sort_order,
                    tabs: ws
                        .tabs
                        .iter()
                        .filter(|tab| {
                            // Exclude hidden worker panes and ephemeral picker panes from persisting across restarts.
                            let pane = self.panes.get(&tab.pane_id);
                            !pane.map(|p| p.hidden || p.skip_snapshot).unwrap_or(false)
                        })
                        .map(|tab| TabSnapshot {
                            id: tab.id.clone(),
                            title: self.display_label_for_pane(&tab.pane_id),
                            description: self
                                .panes
                                .get(&tab.pane_id)
                                .and_then(|pane| pane.description.clone()),
                            split_tree: pane_to_leaf_snapshot(&tab.pane_id, &self.panes),
                            sort_order: tab.sort_order,
                        })
                        .collect(),
                }
            })
            .collect();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        SessionSnapshot {
            version: env!("CARGO_PKG_VERSION").to_string(),
            timestamp,
            workspaces,
            active_workspace_id: self.active_workspace_id.clone(),
        }
    }

    /// Restore state from a session snapshot. Creates workspaces/tabs/panes
    /// without PTYs — frontend will spawn PTYs when panes become visible.
    /// Old snapshots that recorded a multi-pane split tree collapse to the
    /// first leaf; the rest is dropped with a log line since split layouts
    /// are no longer supported.
    pub fn from_session_snapshot(snapshot: &crate::session::store::SessionSnapshot) -> Self {
        use crate::session::store::*;

        /// Walk an old SplitNodeSnapshot looking for the first Leaf. Anything
        /// past the first leaf in a Split branch is discarded — those tabs
        /// will come back as a single-pane after restore.
        fn first_leaf(node: &SplitNodeSnapshot) -> Option<&SplitNodeSnapshot> {
            match node {
                SplitNodeSnapshot::Leaf { .. } => Some(node),
                SplitNodeSnapshot::Split { children, .. } => children.iter().find_map(first_leaf),
            }
        }

        fn install_leaf(
            leaf: &SplitNodeSnapshot,
            legacy_tab_title: &str,
            panes: &mut HashMap<String, Pane>,
        ) -> Option<String> {
            let SplitNodeSnapshot::Leaf {
                pane_id,
                cwd,
                title,
                description,
                title_level,
                agent_type,
                agent_type_locked,
                claude_session_id,
                role,
                role_profile,
                parent_pane_id,
                spawned_by_pane_id,
                report_target_pane_id,
                report_target_mesh_peer,
                can_spawn,
                depth,
                chat_view,
                auto_approve,
                agent_session_model,
                chatbackend_plugin_id,
                chatbackend_session_id,
            } = leaf
            else {
                return None;
            };

            // Re-attach a chatBackend backing so the pane restores as a chat
            // shell (not a PTY). On first send the transport lazily re-opens the
            let restored_backing = match (chatbackend_plugin_id, chatbackend_session_id) {
                (Some(plugin_id), Some(session_id)) => PaneBacking::ChatBackend {
                    plugin_id: plugin_id.clone(),
                    session_id: session_id.clone(),
                },
                _ => PaneBacking::default(),
            };

            let parsed_role = role.as_deref().and_then(|r| match r {
                "Orchestrator" | "Conductor" => {
                    Some(crate::multiplexer::types::AgentRole::Orchestrator)
                }
                "Worker" => Some(crate::multiplexer::types::AgentRole::Worker),
                _ => None,
            });
            let raw_title = title.as_deref().unwrap_or("Terminal");
            let legacy_title = legacy_tab_title.trim();
            let has_agent_type =
                agent_type.is_some() || derive_agent_type_from_title(Some(raw_title)).is_some();
            let restored_title =
                if should_seed_from_legacy_tab_title(raw_title, legacy_title, has_agent_type) {
                    legacy_title.to_string()
                } else {
                    raw_title.to_string()
                };
            let restored_level = match title_level.as_deref() {
                Some("User") => TitleLevel::User,
                Some("Agent") => TitleLevel::Agent,
                Some("Auto") => TitleLevel::Auto,
                Some("Default") => TitleLevel::Default,
                _ => {
                    if role.is_some() {
                        TitleLevel::User
                    } else {
                        TitleLevel::Default
                    }
                }
            };
            let restored_session_id = claude_session_id.as_ref().and_then(|sid| {
                if panes
                    .values()
                    .any(|p| p.agent_session_id.as_deref() == Some(sid.as_str()))
                {
                    None
                } else {
                    Some(sid.clone())
                }
            });
            let restored_agent_type = agent_type
                .clone()
                .or_else(|| derive_agent_type_from_title(title.as_deref()));
            let restored_locked =
                *agent_type_locked || (!*agent_type_locked && restored_agent_type.is_some());
            panes.insert(
                pane_id.clone(),
                Pane {
                    id: pane_id.clone(),
                    pty_id: None,
                    // R3 restore-gap fix: `backing` is persisted now, so a
                    // chatBackend (shell) pane restores as a chatBackend and
                    backing: restored_backing,
                    pane_type: PaneType::Terminal,
                    title: restored_title,
                    description: description.clone(),
                    cwd: cwd.clone(),
                    url: None,
                    agent_type: restored_agent_type,
                    agent_type_locked: restored_locked,
                    agent_session_id: restored_session_id,
                    agent_session_name: None,
                    agent_session_model: agent_session_model.clone(),
                    session_binding_nonce: None,
                    shell_integration_nonce: None,
                    role: parsed_role,
                    role_profile: role_profile.clone(),
                    parent_pane_id: parent_pane_id.clone(),
                    can_spawn: *can_spawn,
                    depth: *depth,
                    // P3: hierarchy map survives restart — restore the real
                    // spawner / report target instead of hardcoding None.
                    report_target_pane_id: report_target_pane_id.clone(),
                    report_target_mesh_peer: report_target_mesh_peer.clone(),
                    spawned_by_pane_id: spawned_by_pane_id.clone(),
                    title_level: restored_level,
                    chat_view: chat_view.unwrap_or(false),
                    pending_prompt: None,
                    agent_state: None,
                    auto_approve: *auto_approve,
                    protected: false,
                    connected_services: vec![],
                    agent_set_at: None,
                    exit_suspected_at: None,
                    agent_booting: false,
                    hidden: false,
                    show_in_tabbar: true,
                    worker_id: None,
                    restored_from_snapshot: true,
                    pending_greeting: None,
                    boot_stage: None,
                },
            );
            Some(pane_id.clone())
        }

        let mut panes = HashMap::new();
        let workspaces: Vec<Workspace> = snapshot.workspaces.iter().map(|ws| {
            Workspace {
                id: ws.id.clone(),
                name: ws.name.clone(),
                active_tab_id: ws.active_tab_id.clone(),
                tabs: ws.tabs.iter().filter_map(|tab| {
                    let leaf = first_leaf(&tab.split_tree)?;
                    let pane_id = install_leaf(leaf, &tab.title, &mut panes)?;
                    if matches!(&tab.split_tree, SplitNodeSnapshot::Split { .. }) {
                        log::warn!(
                            "[restore] tab {} had a multi-pane split — kept the first leaf, dropped the rest",
                            tab.id,
                        );
                    }
                    Some(Tab {
                        id: tab.id.clone(),
                        description: panes
                            .get(&pane_id)
                            .and_then(|pane| pane.description.clone()),
                        pane_id,
                        sort_order: tab.sort_order,
                    })
                }).collect(),
                cwd: ws.cwd.clone(),
                protected: ws.protected,
                sort_order: ws.sort_order,
            }
        }).collect();

        MultiplexerState {
            workspaces,
            panes,
            active_workspace_id: snapshot.active_workspace_id.clone(),
            focused_pane_id: None,
            pty_to_pane: HashMap::new(),
            pane_aliases: HashMap::new(),
            generation: std::sync::atomic::AtomicU64::new(0),
            // Closed history lives in the durable sqlite store (attached at
            // startup), not the session snapshot.
            closed_store: super::ClosedPanesStore::in_memory(),
        }
    }
}

#[cfg(test)]
mod tests;
