use crate::agent::{chat_types::SessionDelta, provider::AgentProvider};
use std::path::Path;

pub fn delta(
    provider: &dyn AgentProvider,
    path: &Path,
    from: u64,
    budget: u64,
) -> Option<SessionDelta> {
    try_delta(provider, path, from, budget).ok().flatten()
}

pub fn try_delta(
    provider: &dyn AgentProvider,
    path: &Path,
    from: u64,
    budget: u64,
) -> Result<Option<SessionDelta>, String> {
    if budget == 0 {
        return Ok(None);
    }
    let Some(source) = provider.record_source("sessionTranscript") else {
        return provider
            .parse_session_delta_bounded(path, from, budget)
            .map(Some)
            .ok_or_else(|| "Could not read session transcript".to_owned());
    };
    let window =
        codeterm_plugin::jsonl_window::records::read_records(path, from, budget, source, None)
            .map_err(|error| error.to_string())?;
    provider.parse_session_records(&window).map(Some)
}

pub fn stored_window(
    provider: &dyn AgentProvider,
    session: &str,
    path: &Path,
    before: Option<u64>,
    budget: u64,
) -> Result<codeterm_plugin::jsonl_window::records::RecordWindow, String> {
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
    let mut window = codeterm_plugin::jsonl_window::records::parse_records_with_limit(
        &bytes,
        start,
        source,
        Some(session),
        budget,
    );
    window.meta.source_key = Some(path.to_string_lossy().into_owned());
    if let Some(reason) = &window.projection_error {
        return Err(reason.clone());
    }
    Ok(window)
}
