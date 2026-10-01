use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::provider::{session_content_hint_term, AgentProvider, SessionMessage, SessionMetadata};
use super::registry::AgentRegistry;
use crate::mem::is_stop_word;

mod scan;

pub(super) const ANY_TERM_CANDIDATE_BUDGET_PER_PROVIDER: usize = 32;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSnippet {
    pub role: String,
    pub text: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ts: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSearchHit {
    pub meta: SessionMetadata,
    pub snippets: Vec<SessionSnippet>,
    pub match_count: usize,
}

/// Per-session scan ceiling: a session file above it is skipped, and a shared
/// store scans at most this many of a session's newest stored bytes.
pub const DEFAULT_SESSION_FILE_BYTE_CAP: u64 = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum SessionMatchMode {
    #[default]
    AllTerms,
    AnyTermRanked,
}

#[derive(Debug, Clone, Default)]
pub struct SessionSearchOpts {
    pub terms: Vec<String>,
    pub match_mode: SessionMatchMode,
    pub provider: Option<String>,
    pub project_path: Option<String>,
    pub limit: usize,
    pub max_snippets: usize,
    /// 0 → `DEFAULT_SESSION_FILE_BYTE_CAP`. Injectable so tests can exercise the
    /// skip branch without materialising a real oversized fixture.
    pub max_file_bytes: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionSkipReason {
    NoContentSource,
    OverByteCap,
    Unreadable,
    ContentTruncated,
    ScanFailed,
}

/// Sessions a provider could not fully search, so an empty result is never
/// mistaken for "no match".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionSearchWarning {
    pub provider: String,
    pub reason: SessionSkipReason,
    pub sessions: usize,
    pub message: String,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SessionSearchResult {
    pub hits: Vec<SessionSearchHit>,
    pub warnings: Vec<SessionSearchWarning>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionSearchError {
    UnknownProvider(String),
}

impl std::fmt::Display for SessionSearchError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownProvider(id) => write!(f, "unknown provider '{id}'"),
        }
    }
}

pub(super) struct ScanCtx {
    lowered_terms: Vec<String>,
    hint_terms: Vec<String>,
    store_hint_terms: Vec<String>,
    match_mode: SessionMatchMode,
    limit: usize,
    max_snippets: usize,
    byte_cap: u64,
    project_path: Option<String>,
}

pub(super) struct ProviderScan {
    provider: String,
    hits: Vec<SessionSearchHit>,
    skipped: Vec<(SessionSkipReason, usize)>,
    scanned: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct SessionSearchStats {
    pub(crate) matched: usize,
    pub(crate) scanned: usize,
}

impl AgentRegistry {
    /// All-terms stays newest-first; any-term ranking has a fixed per-provider budget.
    /// Providers own session reading; incomplete scans are surfaced as warnings.
    pub fn search_sessions(
        &self,
        opts: &SessionSearchOpts,
    ) -> Result<SessionSearchResult, SessionSearchError> {
        self.search_sessions_inner(opts).map(|(result, _)| result)
    }

    pub(crate) fn search_sessions_with_stats(
        &self,
        opts: &SessionSearchOpts,
    ) -> Result<(SessionSearchResult, SessionSearchStats), SessionSearchError> {
        self.search_sessions_inner(opts)
    }

    fn search_sessions_inner(
        &self,
        opts: &SessionSearchOpts,
    ) -> Result<(SessionSearchResult, SessionSearchStats), SessionSearchError> {
        let providers: Vec<Arc<dyn AgentProvider>> = self
            .all_provider_arcs()
            .into_iter()
            .filter(|p| opts.provider.as_deref().is_none_or(|id| id == p.id()))
            .collect();
        if let Some(id) = opts.provider.as_deref() {
            if providers.is_empty() {
                return Err(SessionSearchError::UnknownProvider(id.to_string()));
            }
        }
        if opts.terms.is_empty() {
            return Ok((
                SessionSearchResult::default(),
                SessionSearchStats::default(),
            ));
        }
        let ctx = scan_ctx(opts);
        if ctx.lowered_terms.is_empty() {
            return Ok((
                SessionSearchResult::default(),
                SessionSearchStats::default(),
            ));
        }
        let started = Instant::now();
        let scans: Vec<ProviderScan> = std::thread::scope(|scope| {
            let running: Vec<_> = providers
                .iter()
                .map(|provider| {
                    let ctx = &ctx;
                    let handle = scope.spawn(move || scan::scan_provider(provider.as_ref(), ctx));
                    (provider.id().to_string(), handle)
                })
                .collect();
            running
                .into_iter()
                .map(|(provider, handle)| {
                    handle.join().unwrap_or_else(|_| ProviderScan {
                        provider,
                        hits: Vec::new(),
                        skipped: vec![(SessionSkipReason::ScanFailed, 0)],
                        scanned: 0,
                    })
                })
                .collect()
        });
        let (result, stats) = merge_scans(scans, &ctx);
        log::info!(
            "[sessions-search] total providers={} hits={} warnings={} elapsed_ms={}",
            providers.len(),
            result.hits.len(),
            result.warnings.len(),
            started.elapsed().as_millis()
        );
        Ok((result, stats))
    }
}

fn scan_ctx(opts: &SessionSearchOpts) -> ScanCtx {
    // Positions in an ascii-lowercased copy map 1:1 onto the original bytes;
    // full Unicode lowercasing (`ß → ss`) would misalign the snippet window.
    let mut lowered_terms: Vec<String> = opts
        .terms
        .iter()
        .map(|term| term.to_ascii_lowercase())
        .collect();
    if opts.match_mode == SessionMatchMode::AnyTermRanked {
        let mut seen = HashSet::new();
        lowered_terms
            .retain(|term| !term.is_empty() && !is_stop_word(term) && seen.insert(term.clone()));
    }
    let all_terms_hintable = lowered_terms
        .iter()
        .all(|term| session_content_hint_term(term));
    let safe_file_hints = opts.match_mode != SessionMatchMode::AnyTermRanked || all_terms_hintable;
    let hint_terms: Vec<String> = lowered_terms
        .iter()
        .filter(|term| safe_file_hints && session_content_hint_term(term))
        .cloned()
        .collect();
    let store_hint_terms = if opts.match_mode == SessionMatchMode::AnyTermRanked {
        Vec::new()
    } else {
        hint_terms.clone()
    };
    ScanCtx {
        lowered_terms,
        hint_terms,
        store_hint_terms,
        match_mode: opts.match_mode,
        limit: if opts.limit == 0 { 10 } else { opts.limit },
        max_snippets: if opts.max_snippets == 0 {
            3
        } else {
            opts.max_snippets
        },
        byte_cap: if opts.max_file_bytes == 0 {
            DEFAULT_SESSION_FILE_BYTE_CAP
        } else {
            opts.max_file_bytes
        },
        project_path: opts.project_path.clone(),
    }
}

fn merge_scans(
    scans: Vec<ProviderScan>,
    ctx: &ScanCtx,
) -> (SessionSearchResult, SessionSearchStats) {
    let mut hits = Vec::new();
    let mut warnings = Vec::new();
    let mut stats = SessionSearchStats::default();
    for scan in scans {
        stats.scanned += scan.scanned;
        stats.matched += scan.hits.len();
        hits.extend(scan.hits);
        for (reason, sessions) in scan.skipped {
            warnings.push(SessionSearchWarning {
                message: warning_message(&scan.provider, reason, sessions, ctx.byte_cap),
                provider: scan.provider.clone(),
                reason,
                sessions,
            });
        }
    }
    if ctx.match_mode == SessionMatchMode::AnyTermRanked {
        rank_by_bm25(&mut hits, &ctx.lowered_terms);
    } else {
        hits.sort_by(compare_recency);
    }
    hits.truncate(ctx.limit);
    warnings.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.reason.cmp(&b.reason)));
    (SessionSearchResult { hits, warnings }, stats)
}

fn compare_recency(a: &SessionSearchHit, b: &SessionSearchHit) -> std::cmp::Ordering {
    b.meta
        .modified_ts
        .unwrap_or(0)
        .cmp(&a.meta.modified_ts.unwrap_or(0))
        .then_with(|| a.meta.provider.cmp(&b.meta.provider))
        .then_with(|| a.meta.session_id.cmp(&b.meta.session_id))
}

fn rank_by_bm25(hits: &mut Vec<SessionSearchHit>, terms: &[String]) {
    const K1: f64 = 1.2;
    const B: f64 = 0.75;

    let documents: Vec<(String, usize)> = hits
        .iter()
        .map(|hit| {
            let text = hit
                .snippets
                .iter()
                .map(|snippet| snippet.text.as_str())
                .collect::<Vec<_>>()
                .join(" ")
                .to_ascii_lowercase();
            let length = text
                .split(|character: char| !character.is_alphanumeric())
                .filter(|token| !token.is_empty())
                .count();
            (text, length)
        })
        .collect();
    let document_count = documents.len() as f64;
    let average_length = (documents.iter().map(|(_, length)| *length).sum::<usize>() as f64
        / document_count.max(1.0))
    .max(1.0);
    let document_frequencies: Vec<usize> = terms
        .iter()
        .map(|term| {
            documents
                .iter()
                .filter(|(text, _)| text.contains(term))
                .count()
        })
        .collect();
    let mut scored: Vec<_> = hits
        .drain(..)
        .zip(documents.iter())
        .map(|(hit, (text, length))| {
            let score = terms
                .iter()
                .zip(document_frequencies.iter())
                .map(|(term, frequency)| {
                    let term_frequency = text.matches(term.as_str()).count() as f64;
                    if term_frequency == 0.0 || *frequency == 0 {
                        return 0.0;
                    }
                    let frequency = *frequency as f64;
                    let idf = (1.0 + (document_count - frequency + 0.5) / (frequency + 0.5)).ln();
                    let length_norm = K1 * (1.0 - B + B * *length as f64 / average_length);
                    idf * (term_frequency * (K1 + 1.0)) / (term_frequency + length_norm)
                })
                .sum::<f64>();
            (hit, score)
        })
        .collect();
    scored.sort_by(|(a, score_a), (b, score_b)| {
        score_b
            .total_cmp(score_a)
            .then_with(|| compare_recency(a, b))
    });
    hits.extend(scored.into_iter().map(|(hit, _)| hit));
}

fn warning_message(provider: &str, reason: SessionSkipReason, sessions: usize, cap: u64) -> String {
    let cap = format_bytes(cap);
    match reason {
        SessionSkipReason::NoContentSource => format!(
            "{provider}: {sessions} session(s) not searched: the provider exposes no session content"
        ),
        SessionSkipReason::OverByteCap => {
            format!("{provider}: {sessions} session file(s) over {cap} not searched")
        }
        SessionSkipReason::Unreadable => {
            format!("{provider}: {sessions} session(s) could not be read")
        }
        SessionSkipReason::ContentTruncated => format!(
            "{provider}: {sessions} session(s) over {cap} searched only in their newest {cap}"
        ),
        SessionSkipReason::ScanFailed => format!("{provider}: session scan failed"),
    }
}

fn format_bytes(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB && bytes.is_multiple_of(MIB) {
        format!("{} MiB", bytes / MIB)
    } else {
        format!("{bytes} bytes")
    }
}

pub(super) fn match_messages(
    meta: SessionMetadata,
    messages: &[SessionMessage],
    lowered_terms: &[String],
    max_snippets: usize,
    match_mode: SessionMatchMode,
) -> Option<SessionSearchHit> {
    let mut matched_msgs = 0usize;
    let mut snippets: Vec<SessionSnippet> = Vec::new();
    for msg in messages {
        if msg.is_meta {
            continue;
        }
        let text_lc = msg.text.to_ascii_lowercase();
        let matches = match match_mode {
            SessionMatchMode::AllTerms => lowered_terms.iter().all(|term| text_lc.contains(term)),
            SessionMatchMode::AnyTermRanked => {
                lowered_terms.iter().any(|term| text_lc.contains(term))
            }
        };
        if !matches {
            continue;
        }
        matched_msgs += 1;
        if snippets.len() < max_snippets {
            let pos = lowered_terms
                .iter()
                .filter_map(|t| text_lc.find(t))
                .min()
                .unwrap_or(0);
            let start = floor_char_boundary(&msg.text, pos.saturating_sub(80));
            let end = floor_char_boundary(&msg.text, (pos + 120).min(msg.text.len()));
            let mut snippet = msg.text[start..end].to_string();
            if start > 0 {
                snippet.insert_str(0, "…");
            }
            if end < msg.text.len() {
                snippet.push('…');
            }
            snippets.push(SessionSnippet {
                role: msg.role.clone(),
                text: snippet,
                ts: msg.ts,
            });
        }
    }
    (!snippets.is_empty()).then_some(SessionSearchHit {
        meta,
        snippets,
        match_count: matched_msgs,
    })
}

/// Largest char boundary index ≤ `index`. Shim for the still-unstable
/// `str::floor_char_boundary`; needed to slice UTF-8 safely inside the
/// snippet window.
pub(crate) fn floor_char_boundary(s: &str, index: usize) -> usize {
    let mut i = index.min(s.len());
    while i > 0 && !s.is_char_boundary(i) {
        i -= 1;
    }
    i
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod store_tests;
