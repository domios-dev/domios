use std::collections::{HashMap, HashSet};

use super::{MultiplexerSnapshot, MultiplexerState};
use crate::multiplexer::types::{Tab, TabBacking, TabType, Tab, TitleLevel, Workspace};

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
    tab_title: &str,
    legacy_tab_title: &str,
    has_agent_type: bool,
) -> bool {
    if legacy_tab_title.is_empty() {
        return false;
    }
    let tab_title = tab_title.trim();
    tab_title.is_empty()
        || tab_title == "Terminal"
        || (has_agent_type && derive_agent_type_from_title(Some(tab_title)).is_some())
}

impl MultiplexerState {
    fn live_workspace_tabs(&self) -> Vec<Workspace> {
        let mut claimed_tab_ids = HashSet::new();
        let mut claimed_tab_ids = HashSet::new();

        self.workspaces
            .iter()
            .map(|workspace| {
                let tabs: Vec<Tab> = workspace
                    .tabs
                    .iter()
                    .filter(|tab| {
                        self.tabs.contains_key(&tab.tab_id)
                            && claimed_tab_ids.insert(tab.id.clone())
                            && claimed_tab_ids.insert(tab.tab_id.clone())
                    })
                    .cloned()
                    .collect();
                let active_tab_id = workspace
                    .active_tab_id
                    .as_ref()
                    .filter(|active| tabs.iter().any(|tab| &tab.id == *active))
                    .cloned()
                    .or_else(|| tabs.first().map(|tab| tab.id.clone()));
                Workspace {
                    tabs,
                    active_tab_id,
                    ..workspace.clone()
                }
            })
            .collect()
    }

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
            workspaces: self.live_workspace_tabs(),
            tabs: self.tabs.clone(),
            teams: self.list_teams(),
            active_workspace_id: self.active_workspace_id.clone(),
            focused_tab_id: self.focused_tab_id.clone(),
            generation,
            recently_closed: self.recent_closed_view(),
            tab_statuses: HashMap::new(),
        }
    }

    /// Same as `snapshot()`, plus each tab's live aggregate status — the
    /// snapshot path frontend hydration (`mux_get_state` / `mux_state`) needs
    /// so status survives a page reload instead of staying blank until the
    /// next `tab-status-changed` event.
    ///
    /// A tab with an active `pending_prompt` hydrates as `Waiting` even
    /// though the PTY-output tracker (which only ever sees Working/Idle from
    /// screen-hash churn) never observed it: the live `prompt-state-changed`
    /// path sets `pending_prompt` directly on the tab and never touches
    /// `TabStatusTracker`, so without this override a reload would blank the
    /// attention rail back to whatever the raw output tracker last saw. Takes
    /// priority over everything except a PTY the tracker confirmed `Dead` —
    /// a dead tab can't meaningfully be "waiting".
    pub fn snapshot_with_status(
        &self,
        agent_tracker: &codeterm_types::pty::TabStatusTracker,
    ) -> MultiplexerSnapshot {
        self.snapshot_with_status_resolver(agent_tracker, |_, status| status)
    }

    pub fn snapshot_with_status_resolver(
        &self,
        agent_tracker: &codeterm_types::pty::TabStatusTracker,
        resolve: impl Fn(&Tab, codeterm_types::pty::TabStatus) -> codeterm_types::pty::TabStatus,
    ) -> MultiplexerSnapshot {
        use codeterm_types::pty::TabStatus;

        let mut snap = self.snapshot();
        snap.tab_statuses = self
            .tabs
            .iter()
            .filter_map(|(tab_id, tab)| {
                let tracked = tab
                    .pty_id
                    .as_deref()
                    .and_then(|pty_id| agent_tracker.get_status_if_tracked(pty_id));
                let raw = if tab.pending_prompt.is_some()
                    && !matches!(tracked, Some(TabStatus::Dead))
                {
                    Some(TabStatus::Waiting)
                } else {
                    tracked
                }?;
                Some((tab_id.clone(), resolve(tab, raw)))
            })
            .collect();
        snap
    }

    /// Create a session snapshot for persistence.
    pub fn to_session_snapshot(&self) -> crate::session::store::SessionSnapshot {
        use crate::session::store::*;
        use std::time::{SystemTime, UNIX_EPOCH};

        fn tab_to_leaf_snapshot(
            tab_id: &str,
            tabs: &HashMap<String, Tab>,
        ) -> SplitNodeSnapshot {
            let tab = tabs.get(tab_id);
            let (chatbackend_plugin_id, chatbackend_session_id) = match tab.map(|p| &p.backing) {
                Some(TabBacking::ChatBackend {
                    plugin_id,
                    session_id,
                }) => (Some(plugin_id.clone()), Some(session_id.clone())),
                _ => (None, None),
            };
            SplitNodeSnapshot::Leaf {
                tab_id: tab_id.to_string(),
                cwd: tab.and_then(|p| p.cwd.clone()),
                title: tab.map(|p| p.title.clone()),
                description: tab.and_then(|p| p.description.clone()),
                title_level: tab.map(|p| format!("{:?}", p.title_level)),
                agent_type: tab.and_then(|p| p.agent_type.clone()),
                agent_type_locked: tab.map(|p| p.agent_type_locked).unwrap_or(false),
                claude_session_id: tab.and_then(|p| p.agent_session_id.clone()),
                agent_identity: tab.map(|p| AgentIdentitySnapshot {
                    session_binding_nonce: p.session_binding_nonce.clone(),
                    reported_session_binding_nonce: p.reported_session_binding_nonce.clone(),
                    agent_state: p.agent_state.clone(),
                    boot_stage: p.boot_stage,
                }),
                role: tab.and_then(|p| p.role.map(|r| format!("{:?}", r))),
                role_profile: tab.and_then(|p| p.role_profile.clone()),
                team_id: tab.and_then(|p| p.team_id.clone()),
                parent_tab_id: tab.and_then(|p| p.parent_tab_id.clone()),
                spawned_by_tab_id: tab.and_then(|p| p.spawned_by_tab_id.clone()),
                report_target_tab_id: tab.and_then(|p| p.report_target_tab_id.clone()),
                report_target_mesh_peer: tab.and_then(|p| p.report_target_mesh_peer.clone()),
                can_spawn: tab.map(|p| p.can_spawn).unwrap_or(false),
                depth: tab.map(|p| p.depth).unwrap_or(0),
                chat_view: tab.map(|p| p.chat_view),
                auto_approve: tab.map(|p| p.auto_approve).unwrap_or(false),
                agent_session_model: tab.and_then(|p| p.agent_session_model.clone()),
                agent_planned_model: tab.and_then(|p| p.agent_planned_model.clone()),
                agent_requested_model: tab.and_then(|p| p.agent_requested_model.clone()),
                agent_provider_resolved_model: tab
                    .and_then(|p| p.agent_provider_resolved_model.clone()),
                model_fallback: tab.and_then(|p| p.model_fallback.clone()),
                model_identity_mismatch: tab.and_then(|p| p.model_identity_mismatch.clone()),
                chatbackend_plugin_id,
                chatbackend_session_id,
                watcher: tab.and_then(|p| p.watcher.clone()),
                pending_delivery: tab.and_then(|p| p.pending_delivery.clone()),
            }
        }

        let workspaces = self
            .live_workspace_tabs()
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
                            // Exclude hidden worker tabs and ephemeral picker tabs from persisting across restarts.
                            let tab = self.tabs.get(&tab.tab_id);
                            !tab.map(|p| p.hidden || p.skip_snapshot).unwrap_or(false)
                        })
                        .map(|tab| TabSnapshot {
                            id: tab.id.clone(),
                            title: self.display_label_for_tab(&tab.tab_id),
                            description: self
                                .tabs
                                .get(&tab.tab_id)
                                .and_then(|tab| tab.description.clone()),
                            split_tree: tab_to_leaf_snapshot(&tab.tab_id, &self.tabs),
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
            teams: self.teams.values().cloned().collect(),
        }
    }

    /// Restore state from a session snapshot. Creates workspaces/tabs/tabs
    /// without PTYs — frontend will spawn PTYs when tabs become visible.
    /// Old snapshots that recorded a multi-tab split tree collapse to the
    /// first leaf; the rest is dropped with a log line since split layouts
    /// are no longer supported.
    pub fn from_session_snapshot(snapshot: &crate::session::store::SessionSnapshot) -> Self {
        use crate::session::store::*;

        /// Walk an old SplitNodeSnapshot looking for the first Leaf. Anything
        /// past the first leaf in a Split branch is discarded — those tabs
        /// will come back as a single-tab after restore.
        fn first_leaf(node: &SplitNodeSnapshot) -> Option<&SplitNodeSnapshot> {
            match node {
                SplitNodeSnapshot::Leaf { .. } => Some(node),
                SplitNodeSnapshot::Split { children, .. } => children.iter().find_map(first_leaf),
            }
        }

        fn install_leaf(
            leaf: &SplitNodeSnapshot,
            legacy_tab_title: &str,
            tabs: &mut HashMap<String, Tab>,
        ) -> Option<String> {
            let SplitNodeSnapshot::Leaf {
                tab_id,
                cwd,
                title,
                description,
                title_level,
                agent_type,
                agent_type_locked,
                claude_session_id,
                agent_identity,
                role,
                role_profile,
                team_id,
                parent_tab_id,
                spawned_by_tab_id,
                report_target_tab_id,
                report_target_mesh_peer,
                can_spawn,
                depth,
                chat_view,
                auto_approve,
                agent_session_model,
                agent_planned_model,
                agent_requested_model,
                agent_provider_resolved_model,
                model_fallback,
                model_identity_mismatch,
                chatbackend_plugin_id,
                chatbackend_session_id,
                watcher,
                pending_delivery,
            } = leaf
            else {
                return None;
            };

            let restored_backing = match (chatbackend_plugin_id, chatbackend_session_id) {
                (Some(plugin_id), Some(session_id)) => TabBacking::ChatBackend {
                    plugin_id: plugin_id.clone(),
                    session_id: session_id.clone(),
                },
                _ => TabBacking::default(),
            };

            let parsed_role = role.as_deref().and_then(|r| match r {
                "Orchestrator" | "Conductor" => {
                    Some(crate::multiplexer::types::AgentRole::Orchestrator)
                }
                "Manager" => Some(crate::multiplexer::types::AgentRole::Manager),
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
                if tabs
                    .values()
                    .any(|p| p.agent_session_id.as_deref() == Some(sid.as_str()))
                {
                    None
                } else {
                    Some(sid.clone())
                }
            });
            let restored_identity = restored_session_id.as_ref().and(agent_identity.as_ref());
            let restored_agent_type = agent_type
                .clone()
                .or_else(|| derive_agent_type_from_title(title.as_deref()));
            let restored_locked =
                *agent_type_locked || (!*agent_type_locked && restored_agent_type.is_some());
            tabs.insert(
                tab_id.clone(),
                Tab {
                    id: tab_id.clone(),
                    pty_id: None,
                    backing: restored_backing,
                    tab_type: TabType::Terminal,
                    pending_delivery: pending_delivery.clone(),
                    title: restored_title,
                    description: description.clone(),
                    cwd: cwd.clone(),
                    provisioned_cwd: None,
                    url: None,
                    agent_type: restored_agent_type,
                    agent_type_locked: restored_locked,
                    agent_session_id: restored_session_id,
                    agent_session_name: None,
                    agent_session_model: agent_session_model.clone(),
                    agent_planned_model: agent_planned_model.clone(),
                    agent_requested_model: agent_requested_model.clone(),
                    agent_provider_resolved_model: agent_provider_resolved_model.clone(),
                    model_fallback: model_fallback.clone(),
                    model_identity_mismatch: model_identity_mismatch.clone(),
                    session_binding_nonce: restored_identity
                        .and_then(|identity| identity.session_binding_nonce.clone()),
                    reported_session_binding_nonce: restored_identity
                        .and_then(|identity| identity.reported_session_binding_nonce.clone()),
                    shell_integration_nonce: None,
                    role: parsed_role,
                    role_profile: role_profile.clone(),
                    team_id: team_id.clone(),
                    watcher: watcher.clone(),
                    parent_tab_id: parent_tab_id.clone(),
                    pending_paired_planner_tab_id: None,
                    can_spawn: *can_spawn,
                    depth: *depth,
                    // spawner / report target instead of hardcoding None.
                    report_target_tab_id: report_target_tab_id.clone(),
                    report_target_mesh_peer: report_target_mesh_peer.clone(),
                    spawned_by_tab_id: spawned_by_tab_id.clone(),
                    title_level: restored_level,
                    chat_view: chat_view.unwrap_or(false),
                    pending_prompt: None,
                    agent_state: restored_identity
                        .and_then(|identity| identity.agent_state.clone()),
                    auto_approve: *auto_approve,
                    protected: false,
                    connected_services: vec![],
                    agent_set_at: None,
                    exit_suspected_at: None,
                    agent_booting: false,
                    hidden: false,
                    skip_snapshot: false,
                    show_in_tabbar: true,
                    worker_id: None,
                    restored_from_snapshot: true,
                    pending_greeting: None,
                    boot_stage: restored_identity.and_then(|identity| identity.boot_stage),
                    boot_failure_reason: None,
                },
            );
            Some(tab_id.clone())
        }

        let teams: HashMap<String, codeterm_types::team::Team> = snapshot
            .teams
            .iter()
            .map(|t| (t.id.clone(), t.clone()))
            .collect();

        let mut tabs = HashMap::new();
        let workspaces: Vec<Workspace> = snapshot.workspaces.iter().map(|ws| {
            Workspace {
                id: ws.id.clone(),
                name: ws.name.clone(),
                active_tab_id: ws.active_tab_id.clone(),
                tabs: ws.tabs.iter().filter_map(|tab| {
                    let leaf = first_leaf(&tab.split_tree)?;
                    let tab_id = install_leaf(leaf, &tab.title, &mut tabs)?;
                    if matches!(&tab.split_tree, SplitNodeSnapshot::Split { .. }) {
                        log::warn!(
                            "[restore] tab {} had a multi-tab split — kept the first leaf, dropped the rest",
                            tab.id,
                        );
                    }
                    Some(Tab {
                        id: tab.id.clone(),
                        description: tabs
                            .get(&tab_id)
                            .and_then(|tab| tab.description.clone()),
                        tab_id,
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
            tabs,
            active_workspace_id: snapshot.active_workspace_id.clone(),
            focused_tab_id: None,
            pty_to_tab: HashMap::new(),
            tab_aliases: HashMap::new(),
            teams,
            generation: std::sync::atomic::AtomicU64::new(0),
            // startup), not the session snapshot.
            closed_store: super::ClosedTabsStore::in_memory(),
        }
    }
}

#[cfg(test)]
mod tests;
