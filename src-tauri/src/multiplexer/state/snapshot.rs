use std::collections::HashMap;

use super::{MultiplexerSnapshot, MultiplexerState};
use crate::multiplexer::types::{Pane, PaneBacking, PaneType, Tab, TitleLevel, Workspace};

fn derive_agent_type_from_title(title: Option<&str>) -> Option<String> {
    let t = title?.to_ascii_lowercase();
    for id in ["claude", "codex", "opencode"] {
        if t.contains(id) {
            return Some(id.to_string());
        }
    }
    None
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
            orphaned_closed: self.orphaned_closed.clone(),
        }
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
                parent_pane_id: pane.and_then(|p| p.parent_pane_id.clone()),
                spawned_by_pane_id: pane.and_then(|p| p.spawned_by_pane_id.clone()),
                report_target_pane_id: pane.and_then(|p| p.report_target_pane_id.clone()),
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
                    recently_closed: ws.recently_closed.clone(),
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
                            title: tab.title.clone(),
                            description: self
                                .panes
                                .get(&tab.pane_id)
                                .and_then(|pane| pane.description.clone()),
                            split_tree: pane_to_leaf_snapshot(&tab.pane_id, &self.panes),
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
                parent_pane_id,
                spawned_by_pane_id,
                report_target_pane_id,
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
            // plugin session (R3 restore-gap fix).
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
            let restored_title = title.as_deref().unwrap_or("Terminal").to_string();
            let restored_level = match title_level.as_deref() {
                Some("User") => TitleLevel::User,
                Some("Agent") => TitleLevel::Agent,
                Some("Auto") => TitleLevel::Auto,
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
                    // lazily re-opens its plugin session on first send instead of
                    // wrongly spawning a PTY.
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
                    role: parsed_role,
                    parent_pane_id: parent_pane_id.clone(),
                    can_spawn: *can_spawn,
                    depth: *depth,
                    // P3: hierarchy map survives restart — restore the real
                    // spawner / report target instead of hardcoding None.
                    report_target_pane_id: report_target_pane_id.clone(),
                    spawned_by_pane_id: spawned_by_pane_id.clone(),
                    title_level: restored_level,
                    chat_view: chat_view.unwrap_or(false),
                    pending_prompt: None,
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
                    let pane_id = install_leaf(leaf, &mut panes)?;
                    if matches!(&tab.split_tree, SplitNodeSnapshot::Split { .. }) {
                        log::warn!(
                            "[restore] tab {} had a multi-pane split — kept the first leaf, dropped the rest",
                            tab.id,
                        );
                    }
                    Some(Tab {
                        id: tab.id.clone(),
                        title: tab.title.clone(),
                        description: panes
                            .get(&pane_id)
                            .and_then(|pane| pane.description.clone()),
                        pane_id,
                    })
                }).collect(),
                cwd: ws.cwd.clone(),
                protected: ws.protected,
                recently_closed: ws.recently_closed.clone(),
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
            orphaned_closed: vec![],
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
            parent_pane_id: None,
            spawned_by_pane_id: None,
            report_target_pane_id: None,
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
                recently_closed: vec![],
                tabs: vec![TabSnapshot {
                    id: "tab".into(),
                    title: "tab".into(),
                    description: None,
                    split_tree: leaf,
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
            parent_pane_id: None,
            spawned_by_pane_id: None,
            report_target_pane_id: None,
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
            parent_pane_id: None,
            spawned_by_pane_id: None,
            report_target_pane_id: None,
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
        // erasing a recovered orchestrator's parent/child map.
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
            parent_pane_id: Some("orch-1".into()),
            spawned_by_pane_id: Some("spawner-1".into()),
            report_target_pane_id: Some("target-1".into()),
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
    fn recently_closed_ring_round_trips_through_snapshot() {
        use crate::multiplexer::types::PaneBacking;
        // Close a chatBackend tab + an agent tab, then round-trip the snapshot.
        let (mut state, _root) = MultiplexerState::new();
        let wsid = state.list_workspaces()[0].id.clone();

        let (tab_a, pane_a) = state.create_tab(&wsid).unwrap();
        if let Some(p) = state.get_pane_mut(&pane_a) {
            p.backing = PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "cb-1".into(),
            };
            p.title = "Chat".into();
        }
        state.close_tab(&wsid, &tab_a).unwrap();

        let (_tab_b, pane_b) = state.create_tab(&wsid).unwrap();
        if let Some(p) = state.get_pane_mut(&pane_b) {
            p.agent_type = Some("claude".into());
            p.agent_session_id = Some("ag-9".into());
        }
        state.close_pane(&pane_b).unwrap();

        let snap = state.to_session_snapshot();
        // Persisted on the workspace snapshot, newest first.
        let ws_snap = &snap.workspaces[0];
        assert_eq!(ws_snap.recently_closed.len(), 2);
        assert_eq!(
            ws_snap.recently_closed[0].agent_session_id.as_deref(),
            Some("ag-9")
        );

        let restored = MultiplexerState::from_session_snapshot(&snap);
        let ring = restored.list_recently_closed(&wsid);
        assert_eq!(ring.len(), 2, "ring survives restart");
        assert_eq!(ring[0].agent_type.as_deref(), Some("claude"));
        assert_eq!(
            ring[1].backing,
            PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "cb-1".into(),
            },
            "chatBackend backing survives the snapshot round-trip"
        );

        // And reopen works against the restored ring.
        let mut restored = restored;
        let (_t, np) = restored.reopen_closed_pane(&wsid, None).unwrap();
        assert_eq!(
            restored.get_pane(&np).unwrap().agent_type.as_deref(),
            Some("claude")
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
            parent_pane_id: Some("orch".into()),
            spawned_by_pane_id: Some("spawner".into()),
            report_target_pane_id: None,
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
