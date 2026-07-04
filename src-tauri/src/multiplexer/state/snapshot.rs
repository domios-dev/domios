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
                            // Skip hidden worker panes — they're ephemeral and shouldn't persist across restarts. Prevents ghost tabs in sidebar.
                            !self
                                .panes
                                .get(&tab.pane_id)
                                .map(|p| p.hidden)
                                .unwrap_or(false)
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
mod tests {
    use super::*;
    use crate::session::store::{
        SessionSnapshot, SplitNodeSnapshot, TabSnapshot, WorkspaceSnapshot,
    };

    fn chatbackend_leaf(pane_id: &str, plugin_id: &str, session_id: &str) -> SplitNodeSnapshot {
        SplitNodeSnapshot::Leaf {
            pane_id: pane_id.into(),
            cwd: None,
            title: Some("LM Studio".into()),
            description: None,
            title_level: Some("User".into()),
            agent_type: None,
            agent_type_locked: false,
            claude_session_id: None,
            role: None,
            role_profile: None,
            parent_pane_id: None,
            spawned_by_pane_id: None,
            report_target_pane_id: None,
            report_target_mesh_peer: None,
            can_spawn: false,
            depth: 0,
            chat_view: Some(true),
            auto_approve: false,
            agent_session_model: None,
            chatbackend_plugin_id: Some(plugin_id.into()),
            chatbackend_session_id: Some(session_id.into()),
        }
    }

    fn snapshot_with(leaf: SplitNodeSnapshot) -> SessionSnapshot {
        SessionSnapshot {
            version: "test".into(),
            timestamp: 0,
            active_workspace_id: Some("ws".into()),
            workspaces: vec![WorkspaceSnapshot {
                id: "ws".into(),
                name: "ws".into(),
                active_tab_id: Some("tab".into()),
                cwd: None,
                protected: false,
                sort_order: 0,
                tabs: vec![TabSnapshot {
                    id: "tab".into(),
                    title: "tab".into(),
                    description: None,
                    split_tree: leaf,
                    sort_order: 0,
                }],
            }],
        }
    }

    #[test]
    fn chatbackend_backing_restores_from_snapshot() {
        let snap = snapshot_with(chatbackend_leaf("p1", "lmstudio", "sess-7"));
        let state = MultiplexerState::from_session_snapshot(&snap);
        let pane = state.panes.get("p1").expect("pane restored");
        assert_eq!(
            pane.backing,
            PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "sess-7".into(),
            },
            "restored as chatBackend, not Pty"
        );
        // The frontend (TabContent.tsx) renders the chat UI iff this is true and
        // never mounts a terminal — the "comes back as a terminal" guard.
        assert!(pane.is_chat_backend(), "renders chat, not terminal");
        assert!(pane.restored_from_snapshot, "marked for lazy re-open");
    }

    #[test]
    fn chatbackend_backing_round_trips_through_snapshot() {
        // snapshot → state → snapshot must preserve the chatBackend backing.
        let snap = snapshot_with(chatbackend_leaf("p1", "lmstudio", "sess-7"));
        let state = MultiplexerState::from_session_snapshot(&snap);
        let round = state.to_session_snapshot();

        let leaf = &round.workspaces[0].tabs[0].split_tree;
        match leaf {
            SplitNodeSnapshot::Leaf {
                chatbackend_plugin_id,
                chatbackend_session_id,
                ..
            } => {
                assert_eq!(chatbackend_plugin_id.as_deref(), Some("lmstudio"));
                assert_eq!(chatbackend_session_id.as_deref(), Some("sess-7"));
            }
            _ => panic!("expected a leaf"),
        }
    }

    #[test]
    fn pty_pane_round_trips_without_chatbackend_fields() {
        // A plain PTY pane (agent_type, no backing) must NOT gain a chatBackend
        // backing on round-trip.
        let leaf = SplitNodeSnapshot::Leaf {
            pane_id: "p2".into(),
            cwd: None,
            title: Some("claude".into()),
            description: None,
            title_level: Some("User".into()),
            agent_type: Some("claude".into()),
            agent_type_locked: true,
            claude_session_id: Some("c-1".into()),
            role: None,
            role_profile: None,
            parent_pane_id: None,
            spawned_by_pane_id: None,
            report_target_pane_id: None,
            report_target_mesh_peer: None,
            can_spawn: false,
            depth: 0,
            chat_view: Some(false),
            auto_approve: false,
            agent_session_model: None,
            chatbackend_plugin_id: None,
            chatbackend_session_id: None,
        };
        let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
        let pane = state.panes.get("p2").expect("pane restored");
        assert_eq!(pane.backing, PaneBacking::Pty);
    }

    #[test]
    fn pane_description_round_trips_through_snapshot() {
        let leaf = SplitNodeSnapshot::Leaf {
            pane_id: "p-desc".into(),
            cwd: None,
            title: Some("Worker".into()),
            description: Some("running focused tests".into()),
            title_level: Some("User".into()),
            agent_type: Some("claude".into()),
            agent_type_locked: true,
            claude_session_id: None,
            role: Some("Worker".into()),
            role_profile: Some("planner".into()),
            parent_pane_id: None,
            spawned_by_pane_id: None,
            report_target_pane_id: None,
            report_target_mesh_peer: None,
            can_spawn: false,
            depth: 2,
            chat_view: Some(false),
            auto_approve: false,
            agent_session_model: None,
            chatbackend_plugin_id: None,
            chatbackend_session_id: None,
        };
        let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
        let pane = state.panes.get("p-desc").expect("pane restored");
        assert_eq!(pane.description.as_deref(), Some("running focused tests"));
        assert_eq!(pane.role_profile.as_deref(), Some("planner"));
        assert_eq!(
            state.workspaces[0].tabs[0].description.as_deref(),
            Some("running focused tests")
        );

        let round = state.to_session_snapshot();
        assert_eq!(
            round.workspaces[0].tabs[0].description.as_deref(),
            Some("running focused tests")
        );
        match &round.workspaces[0].tabs[0].split_tree {
            SplitNodeSnapshot::Leaf { description, .. } => {
                assert_eq!(description.as_deref(), Some("running focused tests"));
            }
            _ => panic!("expected a leaf"),
        }
    }

    #[test]
    fn spawned_by_and_report_target_restore_intact_not_none() {
        // Regression for the snapshot.rs gap that hardcoded
        // spawned_by_pane_id: None / report_target_pane_id: None on restore,
        let leaf = SplitNodeSnapshot::Leaf {
            pane_id: "worker-1".into(),
            cwd: None,
            title: Some("Worker".into()),
            description: None,
            title_level: Some("User".into()),
            agent_type: Some("claude".into()),
            agent_type_locked: true,
            claude_session_id: None,
            role: Some("Worker".into()),
            role_profile: None,
            parent_pane_id: Some("orch-1".into()),
            spawned_by_pane_id: Some("spawner-1".into()),
            report_target_pane_id: Some("target-1".into()),
            report_target_mesh_peer: None,
            can_spawn: false,
            depth: 2,
            chat_view: Some(false),
            auto_approve: false,
            agent_session_model: None,
            chatbackend_plugin_id: None,
            chatbackend_session_id: None,
        };
        let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
        let pane = state.panes.get("worker-1").expect("pane restored");
        assert_eq!(
            pane.spawned_by_pane_id.as_deref(),
            Some("spawner-1"),
            "spawned_by must survive restore, not reset to None"
        );
        assert_eq!(
            pane.report_target_pane_id.as_deref(),
            Some("target-1"),
            "report_target must survive restore, not reset to None"
        );
        assert_eq!(pane.parent_pane_id.as_deref(), Some("orch-1"));
    }

    #[test]
    fn legacy_tab_title_seeds_default_provider_pane_title() {
        let leaf = SplitNodeSnapshot::Leaf {
            pane_id: "legacy-worker".into(),
            cwd: None,
            title: Some("claude".into()),
            description: None,
            title_level: Some("Default".into()),
            agent_type: Some("claude".into()),
            agent_type_locked: true,
            claude_session_id: None,
            role: Some("Worker".into()),
            role_profile: None,
            parent_pane_id: None,
            spawned_by_pane_id: None,
            report_target_pane_id: None,
            report_target_mesh_peer: None,
            can_spawn: false,
            depth: 2,
            chat_view: Some(false),
            auto_approve: false,
            agent_session_model: None,
            chatbackend_plugin_id: None,
            chatbackend_session_id: None,
        };
        let mut snap = snapshot_with(leaf);
        snap.workspaces[0].tabs[0].title = "Euler".into();

        let state = MultiplexerState::from_session_snapshot(&snap);
        let pane = state.panes.get("legacy-worker").expect("pane restored");
        assert_eq!(pane.title, "Euler");
        assert_eq!(pane.title_level, TitleLevel::Default);
    }

    #[test]
    fn remote_report_target_round_trips_through_snapshot() {
        // A remote (mesh) worker's report target must survive save+restore so a
        // restarted worker still relays its reports back across the mesh.
        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();
        let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
        if let Some(p) = state.get_pane_mut(&pane_id) {
            p.report_target_pane_id = Some("main".into());
            p.report_target_mesh_peer = Some("Mac-Air".into());
        }
        let snap = state.to_session_snapshot();
        let restored = MultiplexerState::from_session_snapshot(&snap);
        let pane = restored.panes.get(&pane_id).expect("pane restored");
        assert_eq!(pane.report_target_pane_id.as_deref(), Some("main"));
        assert_eq!(
            pane.report_target_mesh_peer.as_deref(),
            Some("Mac-Air"),
            "remote report peer must survive restore"
        );
    }

    #[test]
    fn session_snapshot_no_longer_carries_closed_history() {
        // The closed-tab history moved to the sqlite store; the persisted
        // session snapshot must stay lean and not round-trip it.
        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();
        let (tab_a, _pane_a) = state.create_tab(&wsid).unwrap();
        state.close_tab(&wsid, &tab_a).unwrap();
        assert_eq!(
            state.list_recently_closed(&wsid).len(),
            1,
            "recorded in sqlite"
        );

        let snap = state.to_session_snapshot();
        let json = serde_json::to_value(&snap).unwrap();
        assert!(
            json["workspaces"][0].get("recently_closed").is_none(),
            "session snapshot must not carry the closed-history ring"
        );
    }

    /// A chatBackend pane stays chatBackend after a save + restore cycle.
    /// `pane_chat_backing` returning `Some` is what gates the send path into the
    /// plugin route instead of the PTY route — this invariant must hold after
    /// /new triggers a snapshot re-save (simulated as a full round-trip here).
    #[test]
    fn chatbackend_pane_stays_chat_backend_after_snapshot_resave() {
        // Start with a live chatBackend pane (simulates post-spawn state).
        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();
        let (_, pane_id) = state.create_tab(&wsid).unwrap();
        {
            let pane = state.get_pane_mut(&pane_id).unwrap();
            pane.backing = PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "sess-abc".into(),
            };
            pane.chat_view = true;
        }

        // Save → restore (what happens after /new triggers the 30s save + restart).
        let snap = state.to_session_snapshot();
        let restored = MultiplexerState::from_session_snapshot(&snap);
        let pane = restored.panes.get(&pane_id).expect("pane survived restore");

        // Backing must still be ChatBackend — not Pty — after the round-trip.
        assert!(
            pane.is_chat_backend(),
            "chatBackend pane must stay chatBackend after save+restore; \
             pane_chat_backing() returns None when backing=Pty, causing the \
             send path to mis-route to PTY and throw 'Pane has no PTY'"
        );
        assert_eq!(
            pane.backing,
            PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "sess-abc".into(),
            }
        );
        // Routing invariant: only a ChatBackend backing routes to the plugin.
        // A Pty backing would fall through to ensure_pane_pty_for_send and fail.
        let routes_to_plugin = matches!(pane.backing, PaneBacking::ChatBackend { .. });
        assert!(routes_to_plugin, "send must route to plugin, not PTY path");
    }

    #[test]
    fn snapshot_with_status_mirrors_tracked_pty_status_only() {
        // Regression for the web reload bug: `mux_get_state`/`mux_state` used to
        // carry no status at all, so a reload blanked the tab rail for EVERY
        // status (Working included), not just the pending_prompt/Waiting case.
        // This proves both a Working pane and a Waiting (pending_prompt) pane
        // hydrate their correct rail color straight from the snapshot.
        use crate::agent::status::{PaneStatus, PaneStatusTracker};

        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();
        let (_tab, waiting_pane) = state.create_tab(&wsid).unwrap();
        let (_tab2, working_pane) = state.create_tab(&wsid).unwrap();
        let (_tab3, untracked_pane) = state.create_tab(&wsid).unwrap();
        state.get_pane_mut(&waiting_pane).unwrap().pty_id = Some("pty-waiting".into());
        state.get_pane_mut(&working_pane).unwrap().pty_id = Some("pty-working".into());
        state.get_pane_mut(&untracked_pane).unwrap().pty_id = Some("pty-untracked".into());

        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-waiting", [1u8; 32]);
        tracker.set_waiting("pty-waiting");
        tracker.update_output("pty-working", [2u8; 32]);

        let snap = state.snapshot_with_status(&tracker);
        assert_eq!(
            snap.pane_statuses.get(&waiting_pane),
            Some(&PaneStatus::Waiting),
            "tracked pending_prompt pty's status must be mirrored into the snapshot"
        );
        assert_eq!(
            snap.pane_statuses.get(&working_pane),
            Some(&PaneStatus::Working),
            "tracked working pty's status must be mirrored into the snapshot too"
        );
        assert!(
            !snap.pane_statuses.contains_key(&untracked_pane),
            "a pty the tracker has never seen must be absent, not defaulted to Dead"
        );
    }

    fn test_pending_prompt() -> crate::multiplexer::types::PendingPrompt {
        crate::multiplexer::types::PendingPrompt {
            question: "Allow?".into(),
            options: vec![crate::multiplexer::types::PromptOption {
                label: "Yes".into(),
                value: "1".into(),
            }],
            sections: None,
            multi_select: None,
            submit_available: None,
            selected_values: None,
        }
    }

    #[test]
    fn snapshot_with_status_derives_waiting_from_pending_prompt_over_working() {
        // Regression for the attention-after-reload bug: a hook-driven prompt
        // (POST /hook/permission on a plain shell pane) sets pane.pending_prompt
        // directly and never calls PaneStatusTracker::set_waiting. If the PTY
        // keeps producing output (e.g. a shell loop), update_output() forces the
        // tracker back to Working on every chunk, so a raw tracker mirror alone
        // shows Working after reload even while a prompt is actively pending.
        use crate::agent::status::{PaneStatus, PaneStatusTracker};

        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();
        let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
        {
            let pane = state.get_pane_mut(&pane_id).unwrap();
            pane.pty_id = Some("pty-prompted".into());
            pane.pending_prompt = Some(test_pending_prompt());
        }

        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-prompted", [3u8; 32]);
        assert_eq!(tracker.get_status("pty-prompted"), PaneStatus::Working);

        let snap = state.snapshot_with_status(&tracker);
        assert_eq!(
            snap.pane_statuses.get(&pane_id),
            Some(&PaneStatus::Waiting),
            "pending_prompt must win over a raw Working tracker status in the snapshot"
        );
    }

    #[test]
    fn snapshot_with_status_derives_waiting_from_pending_prompt_when_untracked() {
        // A chatBackend pane (no pty_id) or a pane the tracker has never seen a
        // chunk from yet must still surface Waiting once pending_prompt is set —
        // the tracker's silence must not suppress attention.
        use crate::agent::status::{PaneStatus, PaneStatusTracker};

        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();
        let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
        state.get_pane_mut(&pane_id).unwrap().pending_prompt = Some(test_pending_prompt());

        let tracker = PaneStatusTracker::new();
        let snap = state.snapshot_with_status(&tracker);
        assert_eq!(
            snap.pane_statuses.get(&pane_id),
            Some(&PaneStatus::Waiting),
            "untracked/no-pty pane with a pending_prompt must still hydrate as Waiting"
        );
    }

    #[test]
    fn snapshot_with_status_keeps_dead_over_stale_pending_prompt() {
        // A confirmed-Dead pty must not flip back to Waiting because of a
        // pending_prompt that never got cleared on exit — Dead is terminal.
        use crate::agent::status::{PaneStatus, PaneStatusTracker};

        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();
        let (_tab, pane_id) = state.create_tab(&wsid).unwrap();
        {
            let pane = state.get_pane_mut(&pane_id).unwrap();
            pane.pty_id = Some("pty-dead".into());
            pane.pending_prompt = Some(test_pending_prompt());
        }

        let mut tracker = PaneStatusTracker::new();
        tracker.update_output("pty-dead", [4u8; 32]);
        tracker.set_dead("pty-dead");

        let snap = state.snapshot_with_status(&tracker);
        assert_eq!(
            snap.pane_statuses.get(&pane_id),
            Some(&PaneStatus::Dead),
            "a confirmed-dead pty must stay Dead even with a stale pending_prompt"
        );
    }

    #[test]
    fn spawned_by_round_trips_through_snapshot() {
        // state → snapshot → state preserves the hierarchy linkage fields.
        let leaf = SplitNodeSnapshot::Leaf {
            pane_id: "w".into(),
            cwd: None,
            title: Some("Worker".into()),
            description: None,
            title_level: Some("User".into()),
            agent_type: Some("claude".into()),
            agent_type_locked: true,
            claude_session_id: None,
            role: Some("Worker".into()),
            role_profile: None,
            parent_pane_id: Some("orch".into()),
            spawned_by_pane_id: Some("spawner".into()),
            report_target_pane_id: None,
            report_target_mesh_peer: None,
            can_spawn: false,
            depth: 2,
            chat_view: Some(false),
            auto_approve: false,
            agent_session_model: None,
            chatbackend_plugin_id: None,
            chatbackend_session_id: None,
        };
        let state = MultiplexerState::from_session_snapshot(&snapshot_with(leaf));
        let round = state.to_session_snapshot();
        match &round.workspaces[0].tabs[0].split_tree {
            SplitNodeSnapshot::Leaf {
                spawned_by_pane_id,
                report_target_pane_id,
                ..
            } => {
                assert_eq!(spawned_by_pane_id.as_deref(), Some("spawner"));
                assert_eq!(report_target_pane_id.as_deref(), None);
            }
            _ => panic!("expected a leaf"),
        }
    }
}
