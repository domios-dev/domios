use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use super::{match_messages, ProviderScan, ScanCtx, SessionSearchHit, SessionSkipReason};
use crate::agent::provider::{
    AgentProvider, SessionContentQuery, SessionContentSource, SessionMetadata,
};

/// Worker threads that prefilter one provider's session files.
const FILE_PROBE_WORKERS: usize = 8;
/// A batch is the unit of early exit at `limit`: it starts small so a common
/// term reads a handful of files, then doubles so a rare term keeps every worker busy.
const FIRST_PROBE_BATCH: usize = FILE_PROBE_WORKERS;
const MAX_PROBE_BATCH: usize = FILE_PROBE_WORKERS * 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum FileProbe {
    Parse,
    Miss,
    Skip(SessionSkipReason),
}

#[derive(Default)]
struct ScanTally {
    hits: Vec<SessionSearchHit>,
    skipped: BTreeMap<SessionSkipReason, usize>,
    scanned: usize,
    parsed: usize,
}

impl ScanTally {
    fn skip(&mut self, reason: SessionSkipReason, sessions: usize) {
        *self.skipped.entry(reason).or_default() += sessions;
    }
}

pub(super) fn scan_provider(provider: &dyn AgentProvider, ctx: &ScanCtx) -> ProviderScan {
    let started = Instant::now();
    let source = provider.session_content_source();
    let mut sessions: Vec<SessionMetadata> = provider
        .enumerate_sessions()
        .into_iter()
        .filter(|meta| {
            ctx.project_path
                .as_deref()
                .is_none_or(|proj| meta.project_path.as_deref() == Some(proj))
        })
        .collect();
    let enumerated = sessions.len();
    let enumerate_ms = started.elapsed().as_millis();
    sessions.sort_by_key(|meta| std::cmp::Reverse(meta.modified_ts.unwrap_or(0)));

    let mut tally = ScanTally::default();
    match source {
        SessionContentSource::None if enumerated > 0 => {
            tally.skip(SessionSkipReason::NoContentSource, enumerated)
        }
        SessionContentSource::None => {}
        SessionContentSource::SessionFile => {
            scan_session_files(provider, sessions, ctx, &mut tally)
        }
        SessionContentSource::SharedStore => scan_shared_store(provider, sessions, ctx, &mut tally),
    }
    log::info!(
        "[sessions-search] provider={} source={source:?} sessions={enumerated} scanned={} parsed={} hits={} enumerate_ms={enumerate_ms} elapsed_ms={}",
        provider.id(),
        tally.scanned,
        tally.parsed,
        tally.hits.len(),
        started.elapsed().as_millis()
    );
    ProviderScan {
        provider: provider.id().to_string(),
        hits: tally.hits,
        skipped: tally.skipped.into_iter().collect(),
    }
}

fn scan_shared_store(
    provider: &dyn AgentProvider,
    sessions: Vec<SessionMetadata>,
    ctx: &ScanCtx,
    tally: &mut ScanTally,
) {
    let query = SessionContentQuery {
        max_bytes: ctx.byte_cap,
        hint_terms: ctx.hint_terms.clone(),
    };
    for meta in sessions {
        if tally.hits.len() >= ctx.limit {
            break;
        }
        tally.scanned += 1;
        let Some(content) = provider.read_session_content(&meta.session_id, &query) else {
            tally.skip(SessionSkipReason::Unreadable, 1);
            continue;
        };
        tally.parsed += 1;
        match match_messages(
            meta,
            &content.messages,
            &ctx.lowered_terms,
            ctx.max_snippets,
        ) {
            Some(hit) => tally.hits.push(hit),
            None if content.truncated => tally.skip(SessionSkipReason::ContentTruncated, 1),
            None => {}
        }
    }
}

/// Prefilters a batch of files in parallel on raw bytes, then parses the
/// survivors in recency order, so only candidate files pay for a full parse.
fn scan_session_files(
    provider: &dyn AgentProvider,
    sessions: Vec<SessionMetadata>,
    ctx: &ScanCtx,
    tally: &mut ScanTally,
) {
    let hint_finders: Vec<regex::bytes::Regex> = ctx
        .hint_terms
        .iter()
        .filter_map(|term| regex::bytes::Regex::new(&regex::escape(term)).ok())
        .collect();
    let mut rest = sessions.as_slice();
    let mut batch_len = FIRST_PROBE_BATCH;
    while !rest.is_empty() && tally.hits.len() < ctx.limit {
        let (batch, tail) = rest.split_at(batch_len.min(rest.len()));
        rest = tail;
        batch_len = (batch_len * 2).min(MAX_PROBE_BATCH);
        let probes = probe_batch(batch, ctx.byte_cap, &hint_finders);
        for (meta, probe) in batch.iter().zip(probes) {
            if tally.hits.len() >= ctx.limit {
                break;
            }
            tally.scanned += 1;
            match probe {
                FileProbe::Skip(reason) => tally.skip(reason, 1),
                FileProbe::Miss => {}
                FileProbe::Parse => {
                    tally.parsed += 1;
                    let messages = provider.parse_session_messages(Path::new(&meta.file_path));
                    if let Some(hit) = match_messages(
                        meta.clone(),
                        &messages,
                        &ctx.lowered_terms,
                        ctx.max_snippets,
                    ) {
                        tally.hits.push(hit);
                    }
                }
            }
        }
    }
}

fn probe_batch(
    batch: &[SessionMetadata],
    byte_cap: u64,
    hint_finders: &[regex::bytes::Regex],
) -> Vec<FileProbe> {
    let next = AtomicUsize::new(0);
    let workers = FILE_PROBE_WORKERS.min(batch.len());
    let mut probes = vec![FileProbe::Parse; batch.len()];
    let done: Vec<Vec<(usize, FileProbe)>> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut out = Vec::new();
                    loop {
                        let index = next.fetch_add(1, Ordering::Relaxed);
                        let Some(meta) = batch.get(index) else { break };
                        out.push((index, probe_file(&meta.file_path, byte_cap, hint_finders)));
                    }
                    out
                })
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().unwrap_or_default())
            .collect()
    });
    for (index, probe) in done.into_iter().flatten() {
        probes[index] = probe;
    }
    probes
}

/// Unreadable metadata is not a skip: providers may hand back synthetic paths
/// whose size the filesystem can't report, so those go straight to the parser.
fn probe_file(file_path: &str, byte_cap: u64, hint_finders: &[regex::bytes::Regex]) -> FileProbe {
    let Ok(meta) = std::fs::metadata(file_path) else {
        return FileProbe::Parse;
    };
    if meta.len() > byte_cap {
        return FileProbe::Skip(SessionSkipReason::OverByteCap);
    }
    if hint_finders.is_empty() {
        return FileProbe::Parse;
    }
    let mut bytes = Vec::with_capacity(meta.len() as usize);
    let read =
        std::fs::File::open(file_path).and_then(|file| file.take(byte_cap).read_to_end(&mut bytes));
    if read.is_err() {
        return FileProbe::Skip(SessionSkipReason::Unreadable);
    }
    // A live session that grew past the cap after the stat is parsed, never judged on a prefix.
    if bytes.len() as u64 >= byte_cap {
        return FileProbe::Parse;
    }
    bytes.make_ascii_lowercase();
    if hint_finders.iter().all(|finder| finder.is_match(&bytes)) {
        FileProbe::Parse
    } else {
        FileProbe::Miss
    }
}
