use crate::multiplexer::state::MultiplexerState;
use crate::pty::scrollback_capture::scrollback_eligible;
use crate::session::SessionState;
use crate::OutputBufferState;

pub(crate) enum ScrollbackRestoreOutcome {
    Seeded { bytes: usize },
    Skipped { reason: &'static str },
}

pub(crate) fn load_restored_scrollback_tails(
    mux: &mut MultiplexerState,
    session: &SessionState,
) -> Vec<(String, &'static str)> {
    let pending_ids = mux
        .all_tabs()
        .filter(|tab| {
            tab.scrollback_seed_pending
                && tab.pending_scrollback_tail.is_none()
                && scrollback_eligible(tab)
        })
        .map(|tab| tab.id.clone())
        .collect::<Vec<_>>();
    let tails = {
        let store = session.lock();
        pending_ids
            .iter()
            .map(|tab_id| (tab_id.clone(), store.load_scrollback_tail(tab_id)))
            .collect::<Vec<_>>()
    };
    let mut skipped = Vec::new();
    for (tab_id, tail) in tails {
        let Some(tab) = mux.get_tab_mut(&tab_id) else {
            continue;
        };
        match tail.filter(|tail| !tail.is_empty()) {
            Some(tail) => tab.pending_scrollback_tail = Some(std::sync::Arc::from(tail)),
            None => {
                tab.scrollback_seed_pending = false;
                skipped.push((tab_id, "sidecar_missing_or_empty"));
            }
        }
    }
    skipped
}

pub(crate) fn seed_scrollback_tail_before_link(
    pty_id: &str,
    tail: &[u8],
    has_backend_reader: bool,
    output_buf: &OutputBufferState,
) -> ScrollbackRestoreOutcome {
    if tail.is_empty() {
        return ScrollbackRestoreOutcome::Skipped {
            reason: "sidecar_empty",
        };
    }
    if has_backend_reader {
        return ScrollbackRestoreOutcome::Skipped {
            reason: "backend_reader_already_attached",
        };
    }
    let mut buffer = output_buf.lock();
    if buffer.total_written(pty_id) > 0 {
        return ScrollbackRestoreOutcome::Skipped {
            reason: "output_already_written",
        };
    }
    buffer.prepend(pty_id, tail);
    ScrollbackRestoreOutcome::Seeded { bytes: tail.len() }
}

#[cfg(test)]
mod tests;
