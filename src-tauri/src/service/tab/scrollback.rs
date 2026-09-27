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
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
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
        assert_eq!(buffer.get(&pty_id), Some(tail.as_slice()));
        assert_eq!(
            buffer.read_since(&pty_id, 0),
            (tail.to_vec(), tail.len() as u64)
        );
    }

    #[test]
    #[cfg_attr(not(windows), ignore = "requires a desktop runtime and live PTY")]
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
    #[cfg_attr(
        not(windows),
        ignore = "the shared ServiceContext fixture requires an interactive desktop runtime"
    )]
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
}
