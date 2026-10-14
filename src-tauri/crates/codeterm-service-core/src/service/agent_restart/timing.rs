pub(crate) fn stage(tab: &str, pty: &str, stage: &str) {
    let at_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    log::info!("[restart-timing] tab={tab} stage={stage} pty={pty} at_ms={at_ms}");
}
