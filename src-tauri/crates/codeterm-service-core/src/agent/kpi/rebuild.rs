//! Explicit re-derivation of token usage rows from the transcripts behind every
//! persisted cursor. It runs on the ledger thread, so it never races an ingest.

use super::usage::{UsageIngestor, UsageSource};
use crate::agent::kpi_ledger::{KpiLedger, RebuiltResponse, UsageCursorState, UsageRebuildOutcome};

pub(super) fn rebuild_usage(
    ledger: &KpiLedger,
    ingestor: &mut UsageIngestor,
    providers: Option<&std::sync::Arc<crate::agent::registry::AgentRegistry>>,
) -> Result<UsageRebuildOutcome, String> {
    let mut outcome = UsageRebuildOutcome::default();
    let mut transcript_lines = Vec::new();
    for (path, cursor) in ledger.normalize_usage_cursors()? {
        let source = UsageSource::from_key(&path);
        if !source.path.is_file() {
            outcome.missing_sources += 1;
            outcome.missing_file_sources += 1;
            continue;
        }
        let (batch, reread) = match providers {
            Some(registry) => {
                let Some(owner) = ledger.usage_provider(&path)? else {
                    outcome.missing_sources += 1;
                    outcome.providerless_sources += 1;
                    continue;
                };
                let Some(provider) = registry.provider_arc(&owner) else {
                    outcome.missing_sources += 1;
                    outcome.unavailable_provider_sources += 1;
                    continue;
                };
                let now = super::attribution::now_epoch_secs();
                UsageIngestor::read_through_with_parser(
                    &source,
                    cursor.offset,
                    &|record| provider.parse_usage_record(record, now),
                    provider
                        .usage_sql_query(source.session_id.is_some())
                        .as_deref(),
                )?
            }
            None => UsageIngestor::read_through(&source, cursor.offset)?,
        };
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
    outcome.absorb(ledger.rebuild_runtime_usage()?);
    if outcome.missing_sources > 0 {
        log::warn!("[kpi] usage rebuild skipped {} sources: {} sources with missing files, {} provider-less sources, {} sources with unavailable recorded providers", outcome.missing_sources, outcome.missing_file_sources, outcome.providerless_sources, outcome.unavailable_provider_sources);
    }
    Ok(outcome)
}
