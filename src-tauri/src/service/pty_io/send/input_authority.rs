//! Pre-write input authority for the PTY identity backing an agent tab.

use crate::service::ServiceContext;

/// A replacement PTY reports `AgentReady` on its first rendered frame, which is
/// earlier than its composer owning input. The proven identity is kept per tab,
/// so a rebind must prove authority again instead of inheriting it.
static PROVEN_INPUT_AUTHORITY: std::sync::LazyLock<
    parking_lot::Mutex<std::collections::HashMap<String, String>>,
> = std::sync::LazyLock::new(|| parking_lot::Mutex::new(std::collections::HashMap::new()));

const INPUT_AUTHORITY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);

pub(super) fn input_authority_proven(tab_id: &str, pty_id: &str) -> bool {
    PROVEN_INPUT_AUTHORITY
        .lock()
        .get(tab_id)
        .is_some_and(|proven| proven == pty_id)
}

pub(super) fn record_input_authority(tab_id: &str, pty_id: &str) {
    PROVEN_INPUT_AUTHORITY
        .lock()
        .insert(tab_id.to_string(), pty_id.to_string());
}

/// Ordered pre-write gate. An unproven identity must observe input authority
/// first, and the tab must still be backed by that same identity afterwards.
/// Every refusal happens before the payload is written, so at-most-once holds.
pub(super) fn resolve_replacement_authority(
    tab_id: &str,
    pty_id: &str,
    proven: bool,
    observe_input_authority: impl FnOnce() -> bool,
    bound_pty_after_authority: impl FnOnce() -> Option<String>,
) -> Result<bool, String> {
    let newly_proven = !proven;
    if newly_proven && !observe_input_authority() {
        return Err(format!(
            "Agent in Tab {tab_id} did not take input authority on PTY {pty_id}"
        ));
    }
    if bound_pty_after_authority().as_deref() != Some(pty_id) {
        return Err(format!(
            "Tab {tab_id} rebound away from PTY {pty_id} before the payload was written"
        ));
    }
    Ok(newly_proven)
}

impl ServiceContext {
    pub(crate) fn await_replacement_input_authority(
        &self,
        tab_id: &str,
        pty_id: &str,
    ) -> Result<(), String> {
        let Some(agent_id) = self
            .mux
            .read()
            .get_tab(tab_id)
            .and_then(|tab| tab.agent_type.clone())
        else {
            return Ok(());
        };
        let newly_proven = resolve_replacement_authority(
            tab_id,
            pty_id,
            input_authority_proven(tab_id, pty_id),
            || self.await_agent_tui_on_screen(pty_id, &agent_id, INPUT_AUTHORITY_TIMEOUT),
            || {
                self.mux
                    .read()
                    .get_tab(tab_id)
                    .and_then(|tab| tab.pty_id.clone())
            },
        )?;
        if newly_proven {
            record_input_authority(tab_id, pty_id);
            log::info!(
                "[input-authority] tab={} pty={} observed composer input authority",
                tab_id,
                pty_id
            );
        }
        Ok(())
    }
}
