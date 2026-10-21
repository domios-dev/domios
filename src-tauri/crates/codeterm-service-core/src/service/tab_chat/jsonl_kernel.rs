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
        let started = std::time::Instant::now();
        let result = provider.parse_session_delta_bounded(path, from, budget);
        log::debug!(
            "[chat.jsonl] mode=provider combined_us={} found={}",
            started.elapsed().as_micros(),
            result.is_some()
        );
        return result;
    };
    let started = std::time::Instant::now();
    let window =
        codeterm_plugin::jsonl_window::records::read_records(path, from, budget, source, None)
            .ok()?;
    let read_us = started.elapsed().as_micros();
    let parse_started = std::time::Instant::now();
    let result = provider
        .parse_session_records(&window)
        .map_err(|reason| {
            log::warn!("[jsonl-kernel] session record projection failed: {reason}");
        })
        .ok();
    log::debug!(
        "[chat.jsonl] mode=records read_records_us={} projection_us={} total_us={} found={}",
        read_us,
        parse_started.elapsed().as_micros(),
        started.elapsed().as_micros(),
        result.is_some()
    );
    result
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
