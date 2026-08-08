//! Read-only scrollback capture for plain-shell tabs. Reads the
//! `OutputBuffer` under the documented `mux → output_buf` lock order and
//! returns owned data; it never writes toward PTY input or the write queue.

use std::collections::HashSet;

use codeterm_util::lock_hold::{TimedMutex, TimedRwLock};

use crate::multiplexer::state::MultiplexerState;
use crate::multiplexer::types::{Tab, TabBacking, TabType};

use super::manager::OutputBuffer;

pub struct ScrollbackCapture {
    /// `(tab_id, bounded tail bytes)` for every eligible tab with output.
    pub tails: Vec<(String, Vec<u8>)>,
    /// Every live eligible tab, including restored tabs with no PTY yet —
    /// their sidecars must survive until the frontend replays them.
    pub live_tab_ids: HashSet<String>,
}

/// Plain-shell tabs only: agent tabs restore via provider sessions, chat
/// backends have no PTY scrollback, remote-owned tabs persist on their peer.
pub fn scrollback_eligible(tab: &Tab) -> bool {
    tab.agent_type.is_none()
        && tab.tab_type == TabType::Terminal
        && matches!(tab.backing, TabBacking::Pty)
        && tab.ownership.is_none()
        && !tab.skip_snapshot
}

pub fn capture_scrollback_tails(
    mux: &TimedRwLock<MultiplexerState>,
    output_buf: &TimedMutex<OutputBuffer>,
    limit: usize,
) -> ScrollbackCapture {
    // mux is read before output_buf per the lock order; each guard is scoped
    // so the returned owned data never rides a guard across an await.
    let (plan, live_tab_ids) = {
        let state = mux.read();
        let mut plan = Vec::new();
        let mut live = HashSet::new();
        for tab in state.all_tabs() {
            if !scrollback_eligible(tab) {
                continue;
            }
            live.insert(tab.id.clone());
            if let Some(pty_id) = &tab.pty_id {
                plan.push((tab.id.clone(), pty_id.clone()));
            }
        }
        (plan, live)
    };
    let tails = {
        let buf = output_buf.lock();
        plan.into_iter()
            .filter_map(|(tab_id, pty_id)| {
                let data = buf.get(&pty_id)?;
                if data.is_empty() {
                    return None;
                }
                let start = data.len().saturating_sub(limit);
                Some((tab_id, data[start..].to_vec()))
            })
            .collect()
    };
    ScrollbackCapture {
        tails,
        live_tab_ids,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state_with_tab() -> (MultiplexerState, String) {
        let mut state = MultiplexerState::empty();
        let workspace = state.create_workspace("ws".to_string(), None);
        let tab_id = state.create_tab(&workspace.id).unwrap();
        (state, tab_id)
    }

    fn locked(
        state: MultiplexerState,
        buf: OutputBuffer,
    ) -> (
        TimedRwLock<MultiplexerState>,
        TimedMutex<OutputBuffer>,
    ) {
        (
            TimedRwLock::new("mux", state),
            TimedMutex::new("output_buf", buf),
        )
    }

    #[test]
    fn scrollback_capture_reads_plain_shell_tail() {
        let (mut state, tab_id) = state_with_tab();
        state.link_pty(&tab_id, "pty-1".to_string());
        let mut buf = OutputBuffer::new(65536);
        buf.append("pty-1", b"$ cargo check\r\nok\r\n");
        let (mux, output_buf) = locked(state, buf);

        let capture = capture_scrollback_tails(&mux, &output_buf, 65536);
        assert_eq!(
            capture.tails,
            vec![(tab_id.clone(), b"$ cargo check\r\nok\r\n".to_vec())]
        );
        assert!(capture.live_tab_ids.contains(&tab_id));
    }

    #[test]
    fn scrollback_capture_skips_agent_tabs() {
        let (mut state, tab_id) = state_with_tab();
        state.link_pty(&tab_id, "pty-1".to_string());
        state.get_tab_mut(&tab_id).unwrap().agent_type = Some("claude".into());
        let mut buf = OutputBuffer::new(65536);
        buf.append("pty-1", b"provider-owned output");
        let (mux, output_buf) = locked(state, buf);

        let capture = capture_scrollback_tails(&mux, &output_buf, 65536);
        assert!(capture.tails.is_empty(), "agent tabs get no sidecar");
        assert!(
            !capture.live_tab_ids.contains(&tab_id),
            "an agent tab's stale sidecar must be pruned, not preserved"
        );
    }

    #[test]
    fn scrollback_capture_skips_chatbackend_tabs() {
        let (mut state, tab_id) = state_with_tab();
        state.get_tab_mut(&tab_id).unwrap().backing = TabBacking::ChatBackend {
            plugin_id: "lmstudio".into(),
            session_id: "s1".into(),
        };
        let (mux, output_buf) = locked(state, OutputBuffer::new(65536));

        let capture = capture_scrollback_tails(&mux, &output_buf, 65536);
        assert!(capture.tails.is_empty());
        assert!(!capture.live_tab_ids.contains(&tab_id));
    }

    #[test]
    fn scrollback_capture_bounds_tail_to_limit() {
        let (mut state, tab_id) = state_with_tab();
        state.link_pty(&tab_id, "pty-1".to_string());
        let mut buf = OutputBuffer::new(65536);
        let data: Vec<u8> = (0..4096u32).map(|i| (i % 200) as u8).collect();
        buf.append("pty-1", &data);
        let (mux, output_buf) = locked(state, buf);

        let capture = capture_scrollback_tails(&mux, &output_buf, 1024);
        let (_, tail) = &capture.tails[0];
        assert_eq!(tail.len(), 1024);
        assert_eq!(tail.as_slice(), &data[data.len() - 1024..]);
    }

    #[test]
    fn scrollback_capture_to_restore_round_trip() {
        use crate::session::scrollback::DEFAULT_SCROLLBACK_TAIL_BYTES;
        use crate::session::store::SessionStore;

        let (mut state, tab_id) = state_with_tab();
        state.link_pty(&tab_id, "pty-1".to_string());
        let mut buf = OutputBuffer::new(65536);
        let bytes = b"\x1b[1mline one\x1b[0m\r\nline two\r\n".to_vec();
        buf.append("pty-1", &bytes);
        let (mux, output_buf) = locked(state, buf);

        let dir = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(dir.path().to_path_buf());
        let capture = capture_scrollback_tails(&mux, &output_buf, DEFAULT_SCROLLBACK_TAIL_BYTES);
        for (tab, tail) in &capture.tails {
            store
                .save_scrollback_tail(tab, tail, DEFAULT_SCROLLBACK_TAIL_BYTES)
                .unwrap();
        }
        assert_eq!(store.load_scrollback_tail(&tab_id).unwrap(), bytes);
    }

    #[test]
    fn scrollback_capture_keeps_ptyless_restored_tab_in_live_set() {
        let (state, tab_id) = state_with_tab();
        let (mux, output_buf) = locked(state, OutputBuffer::new(65536));

        let capture = capture_scrollback_tails(&mux, &output_buf, 65536);
        assert!(capture.tails.is_empty());
        assert!(
            capture.live_tab_ids.contains(&tab_id),
            "a restored tab awaiting respawn keeps its sidecar"
        );
    }
}
