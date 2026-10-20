use std::sync::mpsc;
use std::time::{Duration, Instant};

use codeterm_agent_model::agent::provider::SessionMetadata;
use codeterm_agent_model::agent::registry::{SessionSearchHit, SessionSnippet};

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
            provider: "fixture-provider".into(),
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
    assert_eq!(result.hit["provider"], "fixture-provider");
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
