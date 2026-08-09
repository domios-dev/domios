use std::sync::Arc;

use serde::{Deserialize, Serialize};

use super::provider::{AgentProvider, SessionMetadata};
use super::registry::AgentRegistry;

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

/// Per-session read ceiling. Real corpora hold multi-gigabyte rollouts; parsing
/// one to answer a text query is unbounded work, so oversized files are skipped.
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

type Candidate = (Arc<dyn AgentProvider>, SessionMetadata);

impl AgentRegistry {
    /// Cross-provider session search, ordered by recency and bounded by `limit`.
    ///
    /// Metadata for every provider is enumerated first, sorted most-recent-first,
    /// and only then parsed — the scan stops as soon as `limit` hits are
    /// collected, so a common query reads a handful of files instead of the whole
    /// corpus. Files larger than `max_file_bytes` are skipped, never loaded.
    /// Provider handles are cloned up front so no registry lock is held across
    /// disk I/O.
    pub fn search_sessions(&self, opts: &SessionSearchOpts) -> Vec<SessionSearchHit> {
        if opts.terms.is_empty() {
            return Vec::new();
        }
        // Positions in an ascii-lowercased copy map 1:1 onto the original bytes;
        // full Unicode lowercasing (`ß → ss`) would misalign the snippet window.
        let lowered_terms: Vec<String> =
            opts.terms.iter().map(|t| t.to_ascii_lowercase()).collect();
        let limit = if opts.limit == 0 { 10 } else { opts.limit };
        let max_snippets = if opts.max_snippets == 0 {
            3
        } else {
            opts.max_snippets
        };
        let byte_cap = if opts.max_file_bytes == 0 {
            DEFAULT_SESSION_FILE_BYTE_CAP
        } else {
            opts.max_file_bytes
        };

        let mut candidates = self.session_search_candidates(opts);
        candidates.sort_by(|a, b| {
            b.1.modified_ts
                .unwrap_or(0)
                .cmp(&a.1.modified_ts.unwrap_or(0))
        });

        let mut hits: Vec<SessionSearchHit> = Vec::new();
        for (provider, meta) in candidates {
            if hits.len() >= limit {
                break;
            }
            if exceeds_byte_cap(&meta.file_path, byte_cap) {
                continue;
            }
            let matched = match_session(provider.as_ref(), meta, &lowered_terms, max_snippets);
            if let Some(hit) = matched {
                hits.push(hit);
            }
        }
        hits
    }

    fn session_search_candidates(&self, opts: &SessionSearchOpts) -> Vec<Candidate> {
        let mut candidates = Vec::new();
        for provider in self.all_provider_arcs() {
            if let Some(ref pid) = opts.provider {
                if pid != provider.id() {
                    continue;
                }
            }
            for meta in provider.enumerate_sessions() {
                if let Some(ref proj) = opts.project_path {
                    if meta.project_path.as_deref() != Some(proj.as_str()) {
                        continue;
                    }
                }
                candidates.push((provider.clone(), meta));
            }
        }
        candidates
    }
}

/// Unreadable metadata is not a skip: providers may hand back synthetic paths
/// (sqlite-backed history, tests) whose size the filesystem can't report.
fn exceeds_byte_cap(file_path: &str, cap: u64) -> bool {
    std::fs::metadata(file_path)
        .map(|m| m.len() > cap)
        .unwrap_or(false)
}

fn match_session(
    provider: &dyn AgentProvider,
    meta: SessionMetadata,
    lowered_terms: &[String],
    max_snippets: usize,
) -> Option<SessionSearchHit> {
    let messages = provider.parse_session_messages(std::path::Path::new(&meta.file_path));
    let mut matched_msgs = 0usize;
    let mut snippets: Vec<SessionSnippet> = Vec::new();
    for msg in &messages {
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
