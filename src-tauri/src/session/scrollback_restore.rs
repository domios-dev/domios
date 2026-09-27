use crate::multiplexer::commands::MuxState;
use crate::pty::scrollback_capture::scrollback_eligible;
use crate::session::SessionState;
use crate::OutputBufferState;

pub(crate) enum ScrollbackRestoreOutcome {
    Seeded { bytes: usize },
    Skipped { reason: &'static str },
}

pub(crate) fn seed_restored_scrollback_tail(
    tab_id: &str,
    mux: &MuxState,
    output_buf: &OutputBufferState,
    session: &SessionState,
) -> ScrollbackRestoreOutcome {
    let pty_id = {
        let state = mux.read();
        let Some(tab) = state.get_tab(tab_id) else {
            return ScrollbackRestoreOutcome::Skipped {
                reason: "tab_missing_after_materialization",
            };
        };
        if !scrollback_eligible(tab) {
            return ScrollbackRestoreOutcome::Skipped {
                reason: "tab_not_scrollback_eligible",
            };
        }
        let Some(pty_id) = tab.pty_id.clone() else {
            return ScrollbackRestoreOutcome::Skipped {
                reason: "pty_missing_after_materialization",
            };
        };
        pty_id
    };

    let Some(tail) = session.lock().load_scrollback_tail(tab_id) else {
        return ScrollbackRestoreOutcome::Skipped {
            reason: "sidecar_missing_or_invalid",
        };
    };
    if tail.is_empty() {
        return ScrollbackRestoreOutcome::Skipped {
            reason: "sidecar_empty",
        };
    }

    output_buf.lock().prepend(&pty_id, &tail);
    ScrollbackRestoreOutcome::Seeded { bytes: tail.len() }
}
