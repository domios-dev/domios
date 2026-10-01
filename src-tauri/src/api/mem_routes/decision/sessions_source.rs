use std::sync::mpsc::{self, SyncSender, TrySendError};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use serde_json::{json, Value};

use crate::agent::registry::{
    AgentRegistry, SessionMatchMode, SessionSearchHit, SessionSearchOpts,
};
use crate::mem::decision::rerank::RerankCandidate;

#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SessionCandidate {
    pub hit: Value,
    pub candidate: RerankCandidate,
}

type SearchJob = Box<dyn FnOnce() + Send + 'static>;

struct SearchOutput {
    candidates: Vec<SessionCandidate>,
    matched: usize,
    scanned: usize,
}

static SEARCH_WORKER: OnceLock<Result<SyncSender<SearchJob>, String>> = OnceLock::new();

pub(super) fn search(
    registry: Arc<AgentRegistry>,
    query: &str,
    limit: usize,
    max_body_chars: usize,
    deadline: Instant,
) -> Vec<SessionCandidate> {
    if limit == 0 {
        return Vec::new();
    }
    let started = Instant::now();
    if started >= deadline {
        log::debug!(
            "[mem.decision] sessions source status=timeout candidates=0 matched=0 scanned=0 elapsed_ms={}",
            started.elapsed().as_millis()
        );
        return Vec::new();
    }
    let opts = session_search_opts(query, limit);
    let result = run_bounded_search(deadline, move || {
        if Instant::now() >= deadline {
            return Err("deadline".into());
        }
        registry
            .search_sessions_with_stats(&opts)
            .map(|(result, stats)| SearchOutput {
                candidates: candidates_from_hits(result.hits, max_body_chars),
                matched: stats.matched,
                scanned: stats.scanned,
            })
            .map_err(|error| error.to_string())
    });
    let (status, output) = match result {
        Ok(output) => ("applied", output),
        Err(error) => {
            log::debug!("[mem.decision] sessions source skipped: {error}");
            let status = if error == "deadline" {
                "timeout"
            } else {
                "error"
            };
            (
                status,
                SearchOutput {
                    candidates: Vec::new(),
                    matched: 0,
                    scanned: 0,
                },
            )
        }
    };
    log::debug!(
        "[mem.decision] sessions source status={} candidates={} matched={} scanned={} elapsed_ms={}",
        status,
        output.candidates.len(),
        output.matched,
        output.scanned,
        started.elapsed().as_millis()
    );
    output.candidates
}

fn session_search_opts(query: &str, limit: usize) -> SessionSearchOpts {
    SessionSearchOpts {
        terms: query.split_whitespace().map(str::to_owned).collect(),
        match_mode: SessionMatchMode::AnyTermRanked,
        provider: None,
        project_path: None,
        limit,
        max_snippets: 1,
        max_file_bytes: 0,
    }
}

fn run_bounded_search<T, F>(deadline: Instant, search: F) -> Result<T, String>
where
    T: Send + 'static,
    F: FnOnce() -> Result<T, String> + Send + 'static,
{
    let worker = search_worker()?;
    let (reply_tx, reply_rx) = mpsc::sync_channel::<Result<T, String>>(1);
    let job: SearchJob = Box::new(move || {
        let _ = reply_tx.send(search());
    });
    match worker.try_send(job) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => return Err("busy".into()),
        Err(TrySendError::Disconnected(_)) => return Err("worker stopped".into()),
    }
    reply_rx
        .recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .map_err(|error| match error {
            mpsc::RecvTimeoutError::Timeout => String::from("deadline"),
            mpsc::RecvTimeoutError::Disconnected => String::from("worker stopped"),
        })?
}

fn search_worker() -> Result<&'static SyncSender<SearchJob>, String> {
    SEARCH_WORKER
        .get_or_init(|| {
            let (sender, receiver) = mpsc::sync_channel::<SearchJob>(1);
            std::thread::Builder::new()
                .name("mem-decision-sessions".into())
                .spawn(move || {
                    while let Ok(job) = receiver.recv() {
                        job();
                    }
                })
                .map(|_| sender)
                .map_err(|error| error.to_string())
        })
        .as_ref()
        .map_err(Clone::clone)
}

fn candidates_from_hits(
    hits: Vec<SessionSearchHit>,
    max_body_chars: usize,
) -> Vec<SessionCandidate> {
    hits.into_iter()
        .filter_map(|hit| session_candidate(hit, max_body_chars))
        .collect()
}

fn session_candidate(hit: SessionSearchHit, max_body_chars: usize) -> Option<SessionCandidate> {
    let excerpt = hit.snippets.first()?.text.clone();
    let title = hit
        .meta
        .title
        .as_deref()
        .filter(|title| !title.trim().is_empty())
        .unwrap_or("Untitled session");
    let title = truncate_chars(title, max_body_chars).to_string();
    let date = hit
        .meta
        .modified_ts
        .map(|timestamp| format!("Unix timestamp {timestamp}"))
        .unwrap_or_else(|| "unknown".into());
    let excerpt = truncate_chars(&excerpt, max_body_chars).to_string();
    let body = truncate_chars(
        &format!(
            "title: {title}\nprovider: {}\ndate: {date}\nmatched excerpt: {excerpt}",
            hit.meta.provider
        ),
        max_body_chars,
    )
    .to_string();
    let key = format!("session:{}:{}", hit.meta.provider, hit.meta.session_id);
    let candidate = RerankCandidate {
        key,
        title: title.clone(),
        kind: "session".into(),
        body,
    };
    let value = json!({
        "source": "session",
        "session_id": hit.meta.session_id,
        "provider": hit.meta.provider,
        "title": title,
        "excerpt": excerpt,
        "pointer": format!("codeterm sessions get {}", hit.meta.session_id),
    });
    Some(SessionCandidate {
        hit: value,
        candidate,
    })
}

fn truncate_chars(text: &str, max_chars: usize) -> &str {
    match text.char_indices().nth(max_chars) {
        Some((byte, _)) => &text[..byte],
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use crate::agent::provider::SessionMetadata;
    use crate::agent::registry::{SessionSearchHit, SessionSnippet};

    use super::{run_bounded_search, session_candidate, session_search_opts};

    #[test]
    fn decision_sessions_source_uses_any_term_mode() {
        let opts = session_search_opts("release thread", 2);

        assert_eq!(opts.match_mode, super::SessionMatchMode::AnyTermRanked);
        assert_eq!(
            opts.terms,
            vec!["release".to_string(), "thread".to_string()]
        );
    }

    #[test]
    fn candidate_context_is_bounded_and_excludes_provider_file_paths() {
        let hit = SessionSearchHit {
            meta: SessionMetadata {
                session_id: "s-1".into(),
                provider: "claude".into(),
                project_path: None,
                title: Some("Release plan".into()),
                summary: None,
                file_path: "/private/session.jsonl".into(),
                transcript_path: None,
                created_ts: None,
                modified_ts: Some(1_700_000_000),
            },
            snippets: vec![SessionSnippet {
                role: "assistant".into(),
                text: "Use the codeterm sessions get command".into(),
                ts: None,
            }],
            match_count: 1,
        };

        let result = session_candidate(hit, 256).unwrap();

        assert!(result.candidate.body.len() <= 256);
        assert_eq!(result.candidate.kind, "session");
        assert_eq!(result.hit["source"], "session");
        assert_eq!(result.hit["session_id"], "s-1");
        assert_eq!(result.hit["provider"], "claude");
        assert_eq!(result.hit["title"], "Release plan");
        assert_eq!(
            result.hit["excerpt"],
            "Use the codeterm sessions get command"
        );
        assert_eq!(result.hit["pointer"], "codeterm sessions get s-1");
        assert!(!result.hit.to_string().contains("/private/session.jsonl"));
    }

    #[test]
    fn deadline_skips_a_session_search_that_keeps_running() {
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (finished_tx, finished_rx) = mpsc::channel();
        let deadline = Instant::now() + Duration::from_millis(20);
        let result = run_bounded_search(deadline, move || -> Result<Vec<()>, String> {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            finished_tx.send(()).unwrap();
            Ok(Vec::new())
        });

        started_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("session worker started");
        assert_eq!(result.unwrap_err(), "deadline");
        release_tx.send(()).unwrap();
        finished_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("timed-out worker released");
    }
}
