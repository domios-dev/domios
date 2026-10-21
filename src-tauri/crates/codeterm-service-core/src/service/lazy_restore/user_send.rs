use super::{wake_with, ServiceContext, Tab};

#[derive(Debug, PartialEq, Eq)]
enum Preparation {
    WakeRuntime,
    RestoreRuntime,
    WakePty,
    RestorePty,
    None,
}

fn preparation(tab: &Tab, runtime: bool) -> Preparation {
    match (runtime, tab.frozen, tab.restored_from_snapshot) {
        (true, true, _) => Preparation::WakeRuntime,
        (true, false, true) => Preparation::RestoreRuntime,
        (false, true, _) => Preparation::WakePty,
        (false, false, true)
            if matches!(tab.backing, super::TabBacking::Pty) && tab.pty_id.is_none() =>
        {
            Preparation::RestorePty
        }
        _ => Preparation::None,
    }
}

impl ServiceContext {
    pub(crate) fn prepare_user_send_target(&self, tab_id: &str) -> Result<(), String> {
        let runtime = crate::service::agent_runtime::requested_for_tab(self, tab_id);
        let action = {
            let mux = self.mux.read();
            preparation(
                mux.get_tab(tab_id).ok_or("user send tab is absent")?,
                runtime,
            )
        };
        log::debug!("[chat-send] tab={tab_id} admission_preparation={action:?}");
        match action {
            Preparation::WakeRuntime | Preparation::RestoreRuntime => {
                // The durable owner starts the runtime only after admission has conserved the input.
                wake_with(self, tab_id, false, |_, _| Ok(()))?;
                crate::service::agent_runtime::restore::tab(self, tab_id).map(|_| ())
            }
            Preparation::WakePty => self.wake_frozen_tab(tab_id),
            Preparation::RestorePty => self.ensure_tab_pty_for_send(tab_id),
            Preparation::None => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::multiplexer::state::MultiplexerState;

    #[test]
    fn restored_and_slept_input_prepares_admission_without_waiting_for_runtime_readiness() {
        let (mut mux, id) = MultiplexerState::new();
        let tab = mux.get_tab_mut(&id).unwrap();
        assert_eq!(preparation(tab, true), Preparation::None);
        tab.restored_from_snapshot = true;
        assert_eq!(preparation(tab, true), Preparation::RestoreRuntime);
        tab.frozen = true;
        tab.slept_at_ms = Some(42);
        assert_eq!(preparation(tab, true), Preparation::WakeRuntime);
        assert_eq!(preparation(tab, false), Preparation::WakePty);
        tab.frozen = false;
        assert_eq!(preparation(tab, false), Preparation::RestorePty);
        tab.pty_id = Some("already-owned".into());
        assert_eq!(preparation(tab, false), Preparation::None);
    }
}
