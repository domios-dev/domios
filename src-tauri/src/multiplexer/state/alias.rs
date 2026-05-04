use super::MultiplexerState;

impl MultiplexerState {
    /// Resolve a PTY ID to its pane ID (O(1) lookup).
    pub fn pane_id_for_pty(&self, pty_id: &str) -> Option<String> {
        self.pty_to_pane.get(pty_id).cloned()
    }

    /// Link a pane to a PTY. Returns the displaced pty_id so callers can kill
    /// the orphan; `None` means no-op (pane absent or same pty_id).
    pub fn link_pty(&mut self, pane_id: &str, pty_id: String) -> Option<String> {
        let pane = self.panes.get_mut(pane_id)?;
        let displaced = match pane.pty_id.as_ref() {
            Some(old) if old == &pty_id => return None,
            Some(old) => Some(old.clone()),
            None => None,
        };
        if let Some(ref old_pty_id) = displaced {
            self.pty_to_pane.remove(old_pty_id);
        }
        pane.pty_id = Some(pty_id.clone());
        self.pty_to_pane.insert(pty_id, pane_id.to_string());
        displaced
    }

    /// Register a human-readable alias for a pane ID.
    pub fn register_alias(&mut self, alias: &str, pane_id: &str) {
        self.pane_aliases.insert(alias.to_string(), pane_id.to_string());
    }

    /// Remove a pane alias.
    pub fn unregister_alias(&mut self, alias: &str) {
        self.pane_aliases.remove(alias);
    }

    /// Resolve an ID-or-alias to a pane ID. Checks aliases first, then direct pane lookup.
    /// Returns None if neither alias nor direct pane ID matches.
    pub fn resolve_pane_id(&self, id_or_alias: &str) -> Option<String> {
        if let Some(pane_id) = self.pane_aliases.get(id_or_alias) {
            if self.panes.contains_key(pane_id) {
                return Some(pane_id.clone());
            }
        }
        if self.panes.contains_key(id_or_alias) {
            return Some(id_or_alias.to_string());
        }
        None
    }
}
