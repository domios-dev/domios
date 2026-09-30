use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

use super::provider::{session_content_hint_term, AgentProvider, SessionMessage, SessionMetadata};
use super::registry::AgentRegistry;

mod scan;

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

#[derive(Debug, Clone, Default)]
pub struct SessionSearchOpts {
    pub terms: Vec<String>,
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
    limit: usize,
    max_snippets: usize,
    byte_cap: u64,
    project_path: Option<String>,
}

pub(super) struct ProviderScan {
    provider: String,
    hits: Vec<SessionSearchHit>,
    skipped: Vec<(SessionSkipReason, usize)>,
}

impl AgentRegistry {
    /// Cross-provider session search, newest first and bounded by `limit`.
    ///
    /// Each selected provider is scanned on its own thread in recency order and
    /// stops at `limit` hits, so the merged newest `limit` are exact. How a
    /// session is read comes from the provider's `session_content_source`.
    /// Anything a provider could not search is reported as a warning.
    pub fn search_sessions(
        &self,
        opts: &SessionSearchOpts,
    ) -> Result<SessionSearchResult, SessionSearchError> {
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
            return Ok(SessionSearchResult::default());
        }
        let ctx = scan_ctx(opts);
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
                    })
                })
                .collect()
        });
        let result = merge_scans(scans, &ctx);
        log::info!(
            "[sessions-search] total providers={} hits={} warnings={} elapsed_ms={}",
            providers.len(),
            result.hits.len(),
            result.warnings.len(),
            started.elapsed().as_millis()
        );
        Ok(result)
    }
}

fn scan_ctx(opts: &SessionSearchOpts) -> ScanCtx {
    // Positions in an ascii-lowercased copy map 1:1 onto the original bytes;
    // full Unicode lowercasing (`ß → ss`) would misalign the snippet window.
    let lowered_terms: Vec<String> = opts.terms.iter().map(|t| t.to_ascii_lowercase()).collect();
    let hint_terms = lowered_terms
        .iter()
        .filter(|t| session_content_hint_term(t))
        .cloned()
        .collect();
    ScanCtx {
        lowered_terms,
        hint_terms,
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

fn merge_scans(scans: Vec<ProviderScan>, ctx: &ScanCtx) -> SessionSearchResult {
    let mut hits = Vec::new();
    let mut warnings = Vec::new();
    for scan in scans {
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
    hits.sort_by(|a, b| {
        b.meta
            .modified_ts
            .unwrap_or(0)
            .cmp(&a.meta.modified_ts.unwrap_or(0))
            .then_with(|| a.meta.provider.cmp(&b.meta.provider))
            .then_with(|| a.meta.session_id.cmp(&b.meta.session_id))
    });
    hits.truncate(ctx.limit);
    warnings.sort_by(|a, b| a.provider.cmp(&b.provider).then(a.reason.cmp(&b.reason)));
    SessionSearchResult { hits, warnings }
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
) -> Option<SessionSearchHit> {
    let mut matched_msgs = 0usize;
    let mut snippets: Vec<SessionSnippet> = Vec::new();
    for msg in messages {
        if msg.is_meta {
            continue;
        }
        let text_lc = msg.text.to_ascii_lowercase();
        if !lowered_terms.iter().all(|t| text_lc.contains(t)) {
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
