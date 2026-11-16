use codeterm_service_core::agent::ledger::LedgerRow;
pub(super) fn epoch_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}
pub(super) fn ledger_with_live_children(
    mux: &codeterm_service_core::multiplexer::state::MultiplexerState,
    orchestrator_tab_id: &str,
    mut rows: Vec<LedgerRow>,
    now: u64,
) -> Vec<LedgerRow> {
    for child_id in mux.get_children_ids(orchestrator_tab_id) {
        let Some(child) = mux.get_tab(&child_id) else {
            continue;
        };
        let title = mux.display_label_for_tab(&child_id);
        match rows.iter_mut().find(|r| r.worker_tab_id == child_id) {
            Some(row) => {
                row.title = title;
                row.model = child
                    .agent_session_model
                    .clone()
                    .or_else(|| row.model.clone());
                row.updated_at = row.updated_at.max(now);
            }
            None => rows.push(LedgerRow {
                orchestrator_tab_id: orchestrator_tab_id.to_string(),
                worker_tab_id: child_id,
                title,
                model: child.agent_session_model.clone(),
                updated_at: now,
                ..Default::default()
            }),
        }
    }
    rows.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
    rows
}
