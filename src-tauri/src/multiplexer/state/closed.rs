//! Browser-style "recently closed" ring + reopen.
//!
//! On every tab/pane close the multiplexer records a [`ClosedPane`] into the
//! owning workspace's bounded ring (newest first). `reopen_closed_pane`
//! re-materializes the most-recent (or indexed) record into a fresh tab with
//! the SAME backing, marking it `restored_from_snapshot` so the existing
//! restore machinery takes over: a PTY agent resumes its session (or launches
//! fresh if gone), a chatBackend pane lazily reloads its JSONL transcript.

use std::time::{SystemTime, UNIX_EPOCH};

use super::MultiplexerState;
use crate::multiplexer::types::{ClosedPane, TitleLevel};

/// Per-workspace cap on the recently-closed ring. Browser-style: oldest entries
/// fall off the back once exceeded.
pub const RECENTLY_CLOSED_CAP: usize = 20;

fn now_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl MultiplexerState {
    /// Record a pane into its workspace's recently-closed ring, just before it
    /// is removed. No-op for unknown, hidden (ephemeral worker), or protected
    /// panes — those aren't user-closeable tabs and would only pollute the ring.
    pub(super) fn record_closed_pane(&mut self, pane_id: &str) {
        let Some(pane) = self.panes.get(pane_id) else {
            return;
        };
        if pane.hidden || pane.protected {
            return;
        }
        let record = ClosedPane {
            pane_id: pane.id.clone(),
            title: pane.title.clone(),
            description: pane.description.clone(),
            backing: pane.backing.clone(),
            cwd: pane.cwd.clone(),
            agent_type: pane.agent_type.clone(),
            agent_session_id: pane.agent_session_id.clone(),
            model: pane.agent_session_model.clone(),
            closed_at: now_millis(),
        };

        let Some(ws_id) = self.find_workspace_for_pane(pane_id) else {
            return;
        };
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == ws_id) {
            ws.recently_closed.insert(0, record);
            ws.recently_closed.truncate(RECENTLY_CLOSED_CAP);
        }
    }

    /// Snapshot of a workspace's recently-closed ring (newest first).
    pub fn list_recently_closed(&self, workspace_id: &str) -> Vec<ClosedPane> {
        self.get_workspace(workspace_id)
            .map(|ws| ws.recently_closed.clone())
            .unwrap_or_default()
    }

    /// Reopen a recently-closed pane in `workspace_id`. `index` selects the
    /// entry in the ring (newest = 0, the default). Returns `(tab_id, new_pane_id)`
    /// for the freshly created tab. The new pane carries the recorded backing
    /// and agent identity and is flagged `restored_from_snapshot` so the spawn /
    /// lazy-reopen paths resume it exactly like a session restore.
    pub fn reopen_closed_pane(
        &mut self,
        workspace_id: &str,
        index: Option<usize>,
    ) -> Result<(String, String), String> {
        let record = {
            let ws = self
                .workspaces
                .iter_mut()
                .find(|w| w.id == workspace_id)
                .ok_or_else(|| format!("Workspace not found: {}", workspace_id))?;
            if ws.recently_closed.is_empty() {
                return Err("No recently closed panes".to_string());
            }
            let idx = index.unwrap_or(0);
            if idx >= ws.recently_closed.len() {
                return Err(format!(
                    "No recently closed pane at index {} (ring has {})",
                    idx,
                    ws.recently_closed.len()
                ));
            }
            ws.recently_closed.remove(idx)
        };

        let (tab_id, pane_id) = self.create_tab(workspace_id)?;

        if let Some(pane) = self.panes.get_mut(&pane_id) {
            pane.backing = record.backing.clone();
            pane.cwd = record.cwd.clone();
            pane.title = record.title.clone();
            pane.title_level = TitleLevel::User;
            pane.description = record.description.clone();
            pane.agent_type = record.agent_type.clone();
            // Re-lock identity for an agent pane so output-based re-detection
            // can't flip the provider before resume kicks in.
            pane.agent_type_locked = record.agent_type.is_some();
            pane.agent_session_id = record.agent_session_id.clone();
            pane.agent_session_model = record.model.clone();
            // Drive the existing restore path: PTY → resume-or-fresh,
            // chatBackend → lazy transcript reload on first interaction.
            pane.restored_from_snapshot = true;
        }

        // Keep the tab label in sync with the restored pane title + description.
        let _ = self.rename_tab(workspace_id, &tab_id, &record.title);
        if let Some(ws) = self.workspaces.iter_mut().find(|w| w.id == workspace_id) {
            if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == tab_id) {
                tab.description = record.description.clone();
            }
        }

        Ok((tab_id, pane_id))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::types::PaneBacking;

    fn ws_id(state: &MultiplexerState) -> String {
        state.list_workspaces()[0].id.clone()
    }

    #[test]
    fn close_tab_records_pane_into_ring_with_backing() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (tab_id, pane_id) = state.create_tab(&wsid).unwrap();
        // Give the pane a chatBackend backing so we can assert it survives.
        if let Some(p) = state.get_pane_mut(&pane_id) {
            p.backing = PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "s1".into(),
            };
            p.title = "My Chat".into();
        }
        state.close_tab(&wsid, &tab_id).unwrap();

        let ring = state.list_recently_closed(&wsid);
        assert_eq!(ring.len(), 1, "closed tab recorded");
        assert_eq!(ring[0].pane_id, pane_id);
        assert_eq!(ring[0].title, "My Chat");
        assert_eq!(
            ring[0].backing,
            PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "s1".into(),
            }
        );
    }

    #[test]
    fn close_pane_records_agent_identity() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (_tab_id, pane_id) = state.create_tab(&wsid).unwrap();
        if let Some(p) = state.get_pane_mut(&pane_id) {
            p.agent_type = Some("claude".into());
            p.agent_session_id = Some("sess-42".into());
            p.agent_session_model = Some("Opus".into());
            p.cwd = Some("/tmp/work".into());
        }
        state.close_pane(&pane_id).unwrap();

        let ring = state.list_recently_closed(&wsid);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].agent_type.as_deref(), Some("claude"));
        assert_eq!(ring[0].agent_session_id.as_deref(), Some("sess-42"));
        assert_eq!(ring[0].model.as_deref(), Some("Opus"));
        assert_eq!(ring[0].cwd.as_deref(), Some("/tmp/work"));
    }

    #[test]
    fn reopen_recreates_pane_with_same_backing() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (tab_id, pane_id) = state.create_tab(&wsid).unwrap();
        if let Some(p) = state.get_pane_mut(&pane_id) {
            p.backing = PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "s7".into(),
            };
            p.title = "Restored Chat".into();
        }
        state.close_tab(&wsid, &tab_id).unwrap();

        let (new_tab, new_pane) = state.reopen_closed_pane(&wsid, None).unwrap();
        assert_ne!(new_tab, tab_id, "fresh tab id");
        let pane = state.get_pane(&new_pane).expect("reopened pane exists");
        assert_eq!(
            pane.backing,
            PaneBacking::ChatBackend {
                plugin_id: "lmstudio".into(),
                session_id: "s7".into(),
            },
            "same backing on reopen"
        );
        assert!(pane.is_chat_backend());
        assert!(pane.restored_from_snapshot, "drives lazy reopen");
        assert_eq!(pane.title, "Restored Chat");
        // Reopened entry is consumed from the ring.
        assert!(state.list_recently_closed(&wsid).is_empty());
    }

    #[test]
    fn reopen_agent_pane_marks_for_resume() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (_tab_id, pane_id) = state.create_tab(&wsid).unwrap();
        if let Some(p) = state.get_pane_mut(&pane_id) {
            p.agent_type = Some("codex".into());
            p.agent_session_id = Some("cx-1".into());
        }
        state.close_pane(&pane_id).unwrap();

        let (_t, new_pane) = state.reopen_closed_pane(&wsid, None).unwrap();
        let pane = state.get_pane(&new_pane).unwrap();
        assert_eq!(pane.agent_type.as_deref(), Some("codex"));
        assert!(pane.agent_type_locked);
        assert_eq!(pane.agent_session_id.as_deref(), Some("cx-1"));
        assert!(pane.restored_from_snapshot);
        assert_eq!(pane.backing, PaneBacking::Pty);
    }

    #[test]
    fn ring_is_bounded_at_cap() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        for _ in 0..(RECENTLY_CLOSED_CAP + 5) {
            let (tab_id, _pane_id) = state.create_tab(&wsid).unwrap();
            state.close_tab(&wsid, &tab_id).unwrap();
        }
        let ring = state.list_recently_closed(&wsid);
        assert_eq!(ring.len(), RECENTLY_CLOSED_CAP, "ring bounded at cap");
    }

    #[test]
    fn ring_is_newest_first() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (t1, p1) = state.create_tab(&wsid).unwrap();
        state.rename_pane(&p1, "first").unwrap();
        state.close_tab(&wsid, &t1).unwrap();
        let (t2, p2) = state.create_tab(&wsid).unwrap();
        state.rename_pane(&p2, "second").unwrap();
        state.close_tab(&wsid, &t2).unwrap();

        let ring = state.list_recently_closed(&wsid);
        assert_eq!(ring[0].title, "second", "newest first");
        assert_eq!(ring[1].title, "first");
    }

    #[test]
    fn reopen_by_index_selects_older_entry() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (t1, p1) = state.create_tab(&wsid).unwrap();
        state.rename_pane(&p1, "older").unwrap();
        state.close_tab(&wsid, &t1).unwrap();
        let (t2, p2) = state.create_tab(&wsid).unwrap();
        state.rename_pane(&p2, "newer").unwrap();
        state.close_tab(&wsid, &t2).unwrap();

        // index 1 = the older "first"-closed entry.
        let (_t, new_pane) = state.reopen_closed_pane(&wsid, Some(1)).unwrap();
        assert_eq!(state.get_pane(&new_pane).unwrap().title, "older");
        // The remaining ring entry is the newer one.
        let ring = state.list_recently_closed(&wsid);
        assert_eq!(ring.len(), 1);
        assert_eq!(ring[0].title, "newer");
    }

    #[test]
    fn reopen_empty_ring_errors() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let err = state.reopen_closed_pane(&wsid, None).unwrap_err();
        assert!(err.contains("No recently closed"));
    }

    #[test]
    fn reopen_out_of_range_index_errors() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (tab_id, _p) = state.create_tab(&wsid).unwrap();
        state.close_tab(&wsid, &tab_id).unwrap();
        let err = state.reopen_closed_pane(&wsid, Some(5)).unwrap_err();
        assert!(err.contains("index 5"));
    }

    #[test]
    fn hidden_pane_is_not_recorded() {
        let (mut state, _root) = MultiplexerState::new();
        let wsid = ws_id(&state);
        let (tab_id, pane_id) = state.create_tab(&wsid).unwrap();
        if let Some(p) = state.get_pane_mut(&pane_id) {
            p.hidden = true;
        }
        state.close_tab(&wsid, &tab_id).unwrap();
        assert!(
            state.list_recently_closed(&wsid).is_empty(),
            "hidden worker pane must not pollute the ring"
        );
    }
}
