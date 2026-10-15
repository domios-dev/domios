pub fn mark(tab_id: &str, pty_id: Option<&str>, phase: impl std::fmt::Debug) {
    log::debug!(
        "[boot-phase] tab={} pty={} phase={:?} at_us={}",
        tab_id,
        pty_id.unwrap_or("-"),
        phase,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros()
    );
}
