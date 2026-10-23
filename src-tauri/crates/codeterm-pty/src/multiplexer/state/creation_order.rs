use super::MultiplexerState;

pub(super) fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

impl MultiplexerState {
    /// Strictly after every existing stamp so same-millisecond creations keep their order.
    pub(super) fn next_created_at_ms(&self) -> u64 {
        let newest = self
            .workspaces
            .iter()
            .map(|workspace| workspace.created_at_ms)
            .chain(self.tabs.values().map(|tab| tab.created_at_ms))
            .max()
            .unwrap_or(0);
        now_ms().max(newest.saturating_add(1))
    }

    /// Legacy items get their restore order as a stamp once; it predates any real timestamp.
    pub fn backfill_creation_order(&mut self) {
        for (ordinal, workspace) in self.workspaces.iter_mut().enumerate() {
            if workspace.created_at_ms == 0 {
                workspace.created_at_ms = ordinal as u64 + 1;
            }
        }
        let order: Vec<String> = self
            .workspaces
            .iter()
            .flat_map(|workspace| workspace.tabs.iter().cloned())
            .collect();
        let mut next = 1;
        for id in order {
            if let Some(tab) = self.tabs.get_mut(&id) {
                if tab.created_at_ms == 0 {
                    tab.created_at_ms = next;
                }
                next += 1;
            }
        }
        let mut stray: Vec<&mut crate::multiplexer::types::Tab> = self
            .tabs
            .values_mut()
            .filter(|tab| tab.created_at_ms == 0)
            .collect();
        stray.sort_by(|a, b| a.id.cmp(&b.id));
        for tab in stray {
            tab.created_at_ms = next;
            next += 1;
        }
    }
}
