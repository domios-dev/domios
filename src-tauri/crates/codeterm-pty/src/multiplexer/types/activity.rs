impl super::Tab {
    pub fn record_activity(&mut self) {
        self.last_active_at_ms = Some(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .min(u64::MAX as u128) as u64,
        );
    }
}

#[cfg(test)]
mod tests {
    use crate::multiplexer::state::MultiplexerState;

    #[test]
    fn activation_records_activity_under_the_existing_mutation() {
        let (mut mux, tab_id) = MultiplexerState::new();
        let workspace = mux.list_workspaces()[0].id.clone();
        mux.get_tab_mut(&tab_id).unwrap().last_active_at_ms = None;
        mux.set_active_tab(&workspace, &tab_id).unwrap();
        assert!(mux.get_tab(&tab_id).unwrap().last_active_at_ms.is_some());
    }
}
