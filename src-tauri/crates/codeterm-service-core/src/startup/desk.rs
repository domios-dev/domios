use crate::multiplexer::{commands::MuxState, state::MultiplexerSnapshot};

#[derive(Clone)]
pub struct StartupDesk {
    mux: MuxState,
    events: codeterm_events::EventBroadcasterState,
    published: std::sync::Arc<crate::service::PublishedMuxState>,
}

impl StartupDesk {
    pub fn new(mux: MuxState, events: codeterm_events::EventBroadcasterState) -> Self {
        let snapshot = mux.read().snapshot();
        let published = crate::service::PublishedMuxState::new(snapshot.generation, &snapshot)
            .expect("initial desk snapshot serialization");
        Self {
            mux,
            events,
            published: std::sync::Arc::new(published),
        }
    }

    pub fn snapshot(&self) -> MultiplexerSnapshot {
        let snapshot = self.mux.read().snapshot();
        if let Err(error) = self
            .published
            .publish_startup(snapshot.generation, &snapshot)
        {
            log::error!("[startup] desk snapshot publication failed: {error}");
        }
        snapshot
    }

    pub(crate) fn publisher(&self) -> std::sync::Arc<crate::service::PublishedMuxState> {
        self.published.clone()
    }

    pub fn focus_workspace(&self, workspace: &str) -> Result<(), String> {
        self.mux.write().set_active_workspace(workspace)?;
        self.changed();
        Ok(())
    }

    pub fn focus_tab(&self, workspace: &str, tab: &str) -> Result<(), String> {
        {
            let mut mux = self.mux.write();
            mux.set_active_tab(workspace, tab)?;
            mux.set_focused_tab(tab)?;
        }
        self.changed();
        Ok(())
    }

    pub fn focus_pane(&self, tab: &str) -> Result<(), String> {
        self.mux.write().set_focused_tab(tab)?;
        self.changed();
        Ok(())
    }

    fn changed(&self) {
        self.events
            .publish_generic("mux-state-changed", serde_json::json!({}));
    }
}

#[cfg(test)]
mod tests;
