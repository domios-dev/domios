use super::MultiplexerState;

impl MultiplexerState {
    /// Resolve a PTY ID to its tab ID (O(1) lookup).
    pub fn tab_id_for_pty(&self, pty_id: &str) -> Option<String> {
        self.pty_to_tab.get(pty_id).cloned()
    }

    /// Link a tab to a PTY. Returns the displaced pty_id so callers can kill
    /// the orphan; `None` means no-op (tab absent or same pty_id).
    pub fn link_pty(&mut self, tab_id: &str, pty_id: String) -> Option<String> {
        let tab = self.tabs.get_mut(tab_id)?;
        let displaced = match tab.pty_id.as_ref() {
            Some(old) if old == &pty_id => return None,
            Some(old) => Some(old.clone()),
            None => None,
        };
        if let Some(ref old_pty_id) = displaced {
            self.pty_to_tab.remove(old_pty_id);
        }
        tab.pty_id = Some(pty_id.clone());
        self.pty_to_tab.insert(pty_id, tab_id.to_string());
        displaced
    }

    /// Register a human-readable alias for a tab ID.
    pub fn register_alias(&mut self, alias: &str, tab_id: &str) {
        self.tab_aliases
            .insert(alias.to_string(), tab_id.to_string());
    }

    /// Remove a tab alias.
    pub fn unregister_alias(&mut self, alias: &str) {
        self.tab_aliases.remove(alias);
    }

    /// Resolve a public tab ID, internal tab ID, or alias to the backing tab ID.
    pub fn resolve_tab_id(&self, id_or_alias: &str) -> Option<String> {
        if let Some(tab_id) = self.tab_aliases.get(id_or_alias) {
            if self.tabs.contains_key(tab_id) {
                return Some(tab_id.clone());
            }
        }
        if self.tabs.contains_key(id_or_alias) {
            return Some(id_or_alias.to_string());
        }
        for workspace in &self.workspaces {
            if let Some(tab) = workspace.tabs.iter().find(|tab| tab.id == id_or_alias) {
                if self.tabs.contains_key(&tab.tab_id) {
                    return Some(tab.tab_id.clone());
                }
            }
        }
        None
    }
}
