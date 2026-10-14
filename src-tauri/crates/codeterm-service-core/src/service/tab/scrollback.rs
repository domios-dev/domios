use crate::pty::scrollback_capture::scrollback_eligible;
use crate::service::ServiceContext;

enum PendingScrollbackSeed {
    NotPending,
    Ready(std::sync::Arc<[u8]>),
    Skipped(&'static str),
    Deferred(&'static str),
}

impl ServiceContext {
    pub(super) fn seed_pending_scrollback_before_link(&self, tab_id: &str, pty_id: &str) {
        let plan = {
            let mux = self.mux.read();
            let Some(tab) = mux.get_tab(tab_id) else {
                return;
            };
            if !tab.scrollback_seed_pending {
                PendingScrollbackSeed::NotPending
            } else if tab.pty_id.is_some() {
                PendingScrollbackSeed::Deferred("tab_already_linked")
            } else if !scrollback_eligible(tab) {
                PendingScrollbackSeed::Skipped("tab_not_scrollback_eligible")
            } else if let Some(tail) = &tab.pending_scrollback_tail {
                PendingScrollbackSeed::Ready(tail.clone())
            } else {
                PendingScrollbackSeed::Skipped("sidecar_not_loaded")
            }
        };

        let tail = match plan {
            PendingScrollbackSeed::NotPending => return,
            PendingScrollbackSeed::Ready(tail) => tail,
            PendingScrollbackSeed::Skipped(reason) => {
                self.clear_pending_scrollback_seed(tab_id);
                log::debug!("[restore-scrollback] tab={tab_id} skipped reason={reason}");
                return;
            }
            PendingScrollbackSeed::Deferred(reason) => {
                log::warn!("[restore-scrollback] tab={tab_id} seed deferred reason={reason}");
                return;
            }
        };

        let has_reader = self.pty.lock().has_backend_reader(pty_id);
        match crate::session::scrollback_restore::seed_scrollback_tail_before_link(
            pty_id,
            tail.as_ref(),
            has_reader,
            &self.output_buf,
        ) {
            crate::session::scrollback_restore::ScrollbackRestoreOutcome::Seeded { bytes } => {
                self.clear_pending_scrollback_seed(tab_id);
                log::info!("[restore-scrollback] tab={tab_id} seeded bytes={bytes}");
            }
            crate::session::scrollback_restore::ScrollbackRestoreOutcome::Skipped { reason } => {
                log::warn!("[restore-scrollback] tab={tab_id} seed deferred reason={reason}");
            }
        }
    }

    fn clear_pending_scrollback_seed(&self, tab_id: &str) {
        let mut mux = self.mux.write();
        if let Some(tab) = mux.get_tab_mut(tab_id) {
            tab.scrollback_seed_pending = false;
            tab.pending_scrollback_tail = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use crate::multiplexer::state::MultiplexerState;
    use crate::service::test_fixture::ServiceFixture;
    use crate::session::scrollback_restore::load_restored_scrollback_tails;
    use crate::session::store::SessionStore;
    use crate::session::SessionState;
    use parking_lot::Mutex;
    use std::sync::Arc;

    #[test]
    fn restored_shell_tab_seeds_sidecar_into_new_pty_before_attach() {
        let (mux, tab_id) = MultiplexerState::new();
        let mut mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
        let tail = b"restored frontend history\r\n";
        let directory = tempfile::tempdir().unwrap();
        let store = SessionStore::for_test_dir(directory.path().to_path_buf());
        store.save_scrollback_tail(&tab_id, tail, 65_536).unwrap();
        let session: SessionState = Arc::new(Mutex::new(store));
        assert!(load_restored_scrollback_tails(&mut mux, &session).is_empty());
        let fixture = ServiceFixture::new(mux);

        let pty_id = fixture
            .service
            .link_pty(&tab_id, "pty-frontend-first".into());

        let tab = fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .cloned()
            .unwrap();
        assert_eq!(tab.pty_id.as_deref(), Some(pty_id.as_str()));
        assert!(!tab.scrollback_seed_pending);
        let buffer = fixture.service.output_buf.lock();
        let replay = crate::session::scrollback_restore::shell_scrollback_replay(tail);
        assert_eq!(buffer.get(&pty_id), Some(replay.as_slice()));
        assert_eq!(
            buffer.read_since(&pty_id, 0),
            (replay.clone(), replay.len() as u64)
        );
    }

    #[test]
    fn send_first_ensure_seeds_before_starting_the_backend_reader() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tail = b"restored send history\r\n";
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.scrollback_seed_pending = true;
        tab.pending_scrollback_tail = Some(Arc::from(tail.to_vec()));
        let fixture = ServiceFixture::new(mux);

        fixture
            .service
            .ensure_tab_pty_for_send(&tab_id)
            .expect("send-side ensure should materialize the PTY");

        let pty_id = fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .and_then(|tab| tab.pty_id.clone())
            .expect("ensure should link a PTY");
        let tab = fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .cloned()
            .unwrap();
        assert!(!tab.scrollback_seed_pending);
        let buffer = fixture.service.output_buf.lock();
        assert!(buffer.get_full(&pty_id).unwrap().starts_with(tail));
        drop(buffer);
        fixture.service.kill_ptys(&[pty_id]);
    }

    #[test]
    fn existing_stream_offset_is_not_shifted_by_a_deferred_seed() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let pty_id = "pty-stream-already-linked";
        mux.link_pty(&tab_id, pty_id.to_string());
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.scrollback_seed_pending = true;
        tab.pending_scrollback_tail = Some(Arc::from(b"prior history\r\n".to_vec()));
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .output_buf
            .lock()
            .append(pty_id, b"fresh banner\r\n");
        let held_offset = fixture.service.output_buf.lock().total_written(pty_id);

        fixture.service.link_pty(&tab_id, pty_id.to_string());

        let buffer = fixture.service.output_buf.lock();
        assert_eq!(buffer.total_written(pty_id), held_offset);
        assert_eq!(
            buffer.read_since(pty_id, held_offset),
            (Vec::new(), held_offset)
        );
        drop(buffer);
        let tab = fixture
            .service
            .mux
            .read()
            .get_tab(&tab_id)
            .cloned()
            .unwrap();
        assert!(tab.scrollback_seed_pending);
        assert_eq!(
            tab.pending_scrollback_tail.as_deref(),
            Some(&b"prior history\r\n"[..])
        );
    }

    #[test]
    fn agent_restore_preserves_full_screen_repaint_without_shell_scrolling() {
        let (mut mux, tab_id) = MultiplexerState::new();
        mux.get_tab_mut(&tab_id).unwrap().agent_type = Some("test-tui".into());
        let mut mux = MultiplexerState::from_session_snapshot(&mux.to_session_snapshot());
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        assert!(!tab.scrollback_seed_pending);
        tab.scrollback_seed_pending = true;
        tab.pending_scrollback_tail = Some(Arc::from(b"stale shell sidecar".to_vec()));
        let fixture = ServiceFixture::new(mux);
        let pty_id = fixture.service.link_pty(&tab_id, "pty-tui-restore".into());
        let replay = b"\x1b[?1049h\x1b[Hprevious frame";
        let frame = b"\x1b[2J\x1b[Hnew frame\x1b[6;1Hcomposer> ";
        let mut buffer = fixture.service.output_buf.lock();
        assert_eq!(buffer.total_written(&pty_id), 0);
        buffer.append(&pty_id, replay);
        buffer.append(&pty_id, frame);
        let mut expected = replay.to_vec();
        expected.extend_from_slice(frame);
        assert_eq!(buffer.get_full(&pty_id), Some(expected.as_slice()));
        let mut parser = vt100::Parser::new(6, 80, 256);
        parser.process(buffer.get_full(&pty_id).unwrap());
        assert_eq!(
            parser.screen().rows(0, 80).collect::<Vec<_>>(),
            ["new frame", "", "", "", "", "composer> "]
        );
        assert_eq!(parser.screen().cursor_position(), (5, 10));
        parser.screen_mut().set_scrollback(usize::MAX);
        assert_eq!(parser.screen().scrollback(), 0);
    }

    #[test]
    fn chat_backend_restore_never_seeds_shell_viewport_bytes() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&tab_id).unwrap();
        tab.backing = crate::multiplexer::types::TabBacking::ChatBackend {
            plugin_id: "test-chat".into(),
            session_id: "session".into(),
        };
        tab.scrollback_seed_pending = true;
        tab.pending_scrollback_tail = Some(Arc::from(b"stale shell sidecar".to_vec()));
        let fixture = ServiceFixture::new(mux);
        fixture
            .service
            .seed_pending_scrollback_before_link(&tab_id, "unused-pty");
        assert_eq!(
            fixture
                .service
                .output_buf
                .lock()
                .total_written("unused-pty"),
            0
        );
        assert!(
            !fixture
                .service
                .mux
                .read()
                .get_tab(&tab_id)
                .unwrap()
                .scrollback_seed_pending
        );
    }
}
