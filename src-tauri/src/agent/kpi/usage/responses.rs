//! Folds parsed usage lines into one record per provider response. A record's key is
//! the ledger's dedup key, so a range read twice can only merge into the rows it made.

use std::collections::HashMap;

use super::records::ParsedUsage;
use crate::agent::kpi_ledger::{CumulativeUsage, TokenCounts};

/// `tokens: None` is a context-compaction marker: an event with no token cost of its own.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UsageRecord {
    pub timestamp: u64,
    pub key: String,
    pub tokens: Option<TokenCounts>,
    /// `(timestamp, tokens)` as each folded line reported it, one entry per line.
    pub observed: Vec<(u64, TokenCounts)>,
}

/// Where a source's running totals stand at its cursor. `Unknown` is a watermark saved
/// without totals, so the first cumulative line there counts only its own last turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CumulativeBaseline {
    #[default]
    Start,
    Known(CumulativeUsage),
    Unknown,
}

impl CumulativeBaseline {
    pub fn resumed(offset: u64, totals: Option<CumulativeUsage>) -> Self {
        match (offset, totals) {
            (_, Some(totals)) => CumulativeBaseline::Known(totals),
            (0, None) => CumulativeBaseline::Start,
            _ => CumulativeBaseline::Unknown,
        }
    }

    pub fn totals(self) -> Option<CumulativeUsage> {
        match self {
            CumulativeBaseline::Known(totals) => Some(totals),
            CumulativeBaseline::Start | CumulativeBaseline::Unknown => None,
        }
    }
}

pub(super) struct ResponseCollector<'a> {
    source: &'a str,
    baseline: CumulativeBaseline,
    records: Vec<UsageRecord>,
    by_key: HashMap<String, usize>,
    superseded: Vec<(u64, TokenCounts)>,
}

pub(super) struct CollectedUsage {
    pub records: Vec<UsageRecord>,
    pub superseded: Vec<(u64, TokenCounts)>,
    pub baseline: CumulativeBaseline,
}

impl<'a> ResponseCollector<'a> {
    pub fn new(source: &'a str, baseline: CumulativeBaseline) -> Self {
        Self {
            source,
            baseline,
            records: Vec::new(),
            by_key: HashMap::new(),
            superseded: Vec::new(),
        }
    }

    /// `position` is the line's byte offset or the row's rowid, which identifies a record
    /// whose provider names no response.
    pub fn push(&mut self, parsed: ParsedUsage, position: u64) {
        match parsed {
            ParsedUsage::Compaction { timestamp } => {
                let key = format!("usage:compaction:{}:{position}", self.source);
                self.merge(key, timestamp, None, None);
            }
            ParsedUsage::Response {
                timestamp,
                response_id,
                tokens,
            } => {
                let key = match response_id {
                    Some(id) => format!("usage:response:{id}"),
                    None => format!("usage:line:{}:{position}", self.source),
                };
                self.merge(key, timestamp, Some(tokens), Some(tokens));
            }
            ParsedUsage::Cumulative {
                timestamp,
                total,
                last,
            } => self.push_cumulative(timestamp, total, last),
        }
    }

    fn push_cumulative(
        &mut self,
        timestamp: u64,
        total: CumulativeUsage,
        last: Option<TokenCounts>,
    ) {
        let delta = match self.baseline {
            CumulativeBaseline::Start => Some(total.as_tokens()),
            CumulativeBaseline::Known(previous) => total.advance_from(previous),
            CumulativeBaseline::Unknown => Some(last.unwrap_or_else(|| total.as_tokens())),
        };
        self.baseline = CumulativeBaseline::Known(total);
        let Some(tokens) = delta else {
            self.superseded.extend(last.map(|line| (timestamp, line)));
            return;
        };
        let key = format!(
            "usage:cumulative:{}:{}:{}:{}:{}",
            self.source, total.total, total.input, total.cached_input, total.output
        );
        self.merge(key, timestamp, Some(tokens), last);
    }

    fn merge(
        &mut self,
        key: String,
        timestamp: u64,
        tokens: Option<TokenCounts>,
        observed: Option<TokenCounts>,
    ) {
        let observed = observed.map(|line| (timestamp, line));
        if let Some(&index) = self.by_key.get(&key) {
            let record = &mut self.records[index];
            record.tokens = match (record.tokens, tokens) {
                (Some(held), Some(next)) => Some(held.max_each(next)),
                (held, next) => held.or(next),
            };
            record.observed.extend(observed);
            return;
        }
        self.by_key.insert(key.clone(), self.records.len());
        self.records.push(UsageRecord {
            timestamp,
            key,
            tokens,
            observed: observed.into_iter().collect(),
        });
    }

    pub fn finish(self) -> CollectedUsage {
        CollectedUsage {
            records: self.records,
            superseded: self.superseded,
            baseline: self.baseline,
        }
    }
}
