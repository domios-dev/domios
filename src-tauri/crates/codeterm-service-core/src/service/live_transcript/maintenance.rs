use std::sync::Arc;

use super::{SegmentSummary, TranscriptContent, TranscriptStore};
use crate::service::ServiceContext;

const JOBS_PER_EVENT: usize = 4;
const MAX_SUMMARY_CHARS: usize = 1600;

#[derive(Default)]
struct Maintenance(parking_lot::Mutex<()>);

pub(super) fn schedule(svc: &Arc<ServiceContext>, store: TranscriptStore) {
    let svc = svc.clone();
    tokio::task::spawn_blocking(move || {
        let Some(_running) = svc.ext_or_init(Maintenance::default).0.try_lock() else {
            return;
        };
        if summarize(&store).is_err() {
            log::debug!("[live-transcript] sealed summary job did not complete");
        }
    });
}

pub(super) fn summarize(store: &TranscriptStore) -> std::io::Result<()> {
    for range in store.pending_summaries(JOBS_PER_EVENT)? {
        let rows = store.read_sealed(range)?;
        let speech: Vec<_> = rows
            .iter()
            .filter_map(|row| match &row.content {
                TranscriptContent::Speech { text } if !text.trim().is_empty() => {
                    Some(text.as_str())
                }
                _ => None,
            })
            .collect();
        let mut text = format!(
            "{} speech entries, {} capture markers.\n",
            speech.len(),
            rows.len() - speech.len()
        );
        for sample in speech
            .iter()
            .take(3)
            .chain(speech.iter().skip(3).rev().take(1))
        {
            let bounded: String = sample.trim().chars().take(MAX_SUMMARY_CHARS / 4).collect();
            text.push_str(&bounded);
            text.push('\n');
        }
        store.write_summary(SegmentSummary { range, text })?;
    }
    store.evict_to_budget()?;
    Ok(())
}
