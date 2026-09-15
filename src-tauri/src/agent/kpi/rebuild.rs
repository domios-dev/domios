//! Explicit re-derivation of token usage rows from the transcripts behind every
//! persisted cursor. It runs on the ledger thread, so it never races an ingest.

use super::usage::{UsageIngestor, UsageSource};
use crate::agent::kpi_ledger::{KpiLedger, RebuiltResponse, UsageCursorState, UsageRebuildOutcome};

pub(super) fn rebuild_usage(
    ledger: &KpiLedger,
    ingestor: &mut UsageIngestor,
) -> Result<UsageRebuildOutcome, String> {
    let mut outcome = UsageRebuildOutcome::default();
    let mut transcript_lines = Vec::new();
    for (path, cursor) in ledger.list_usage_cursors()? {
        let source = UsageSource::new(path.as_str());
        if !source.path.is_file() {
            outcome.missing_sources += 1;
            continue;
        }
        let (batch, reread) = UsageIngestor::read_through(&source, cursor.offset)?;
        transcript_lines.extend(
            batch
                .records
                .iter()
                .flat_map(|record| record.observed.iter().copied()),
        );
        transcript_lines.extend(batch.superseded.iter().copied());
        let responses: Vec<RebuiltResponse> = batch
            .records
            .into_iter()
            .filter_map(|record| {
                Some(RebuiltResponse {
                    tokens: record.tokens?,
                    key: record.key,
                    timestamp: record.timestamp,
                    observed: record.observed,
                })
            })
            .collect();
        let committed = UsageCursorState {
            offset: cursor.offset,
            cumulative: reread.persisted().cumulative,
        };
        outcome.absorb(ledger.rebuild_usage_source(
            &path,
            &responses,
            &batch.superseded,
            committed,
        )?);
        ingestor.forget(&source);
    }
    outcome.legacy_rows_removed += ledger.remove_duplicate_legacy_rows(&transcript_lines)?;
    Ok(outcome)
}
