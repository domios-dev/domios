use crate::agent::{chat_types::SessionDelta, provider::AgentProvider};
use std::path::Path;

pub fn delta(
    provider: &dyn AgentProvider,
    path: &Path,
    from: u64,
    budget: u64,
) -> Option<SessionDelta> {
    if budget == 0 {
        return None;
    }
    let Some(source) = provider.record_source("sessionTranscript") else {
        return provider.parse_session_delta_bounded(path, from, budget);
    };
    let window =
        codeterm_plugin::jsonl_window::records::read_records(path, from, budget, source, None)
            .ok()?;
    provider
        .parse_session_records(&window)
        .map_err(|reason| {
            log::warn!("[jsonl-kernel] session record projection failed: {reason}");
        })
        .ok()
}

pub fn stored(
    provider: &dyn AgentProvider,
    session: &str,
    path: &Path,
    before: Option<u64>,
    budget: u64,
) -> Result<(Vec<crate::agent::chat_types::ChatMsg>, u64, u64), String> {
    let source = provider
        .record_source("storedHistory")
        .ok_or("Stored record source is undeclared")?;
    let total = std::fs::metadata(path)
        .map_err(|error| error.to_string())?
        .len();
    let page = codeterm_plugin::jsonl_window::page_before(
        path,
        before.unwrap_or(total).min(total),
        budget,
    )
    .map_err(|error| error.to_string())?;
    if page.end > 0 && page.start >= page.end {
        return Err("Stored history record exceeds backward scan limit".into());
    }
    let (bytes, start) = codeterm_plugin::jsonl_window::read_range(path, page.start, page.end)
        .map_err(|error| error.to_string())?;
    let window = codeterm_plugin::jsonl_window::records::parse_records_with_limit(
        &bytes,
        start,
        source,
        Some(session),
        budget,
    );
    let rows = provider.runtime_stored_records(session, &window)?;
    Ok((rows, start, window.meta.new_offset))
}
