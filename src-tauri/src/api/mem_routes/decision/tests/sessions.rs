use super::*;
use crate::mem::decision::rerank::RerankCandidate;

fn session_candidate() -> SessionCandidate {
    SessionCandidate {
        hit: json!({
            "source": "session",
            "session_id": "sess-42",
            "provider": "claude",
            "title": "Release command",
            "excerpt": "Use codeterm sessions get sess-42 to inspect the prior run.",
            "pointer": "codeterm sessions get sess-42",
        }),
        candidate: RerankCandidate {
            key: "session:claude:sess-42".into(),
            title: "Release command".into(),
            kind: "session".into(),
            body: "title: Release command\nprovider: claude\ndate: Unix timestamp 1\nmatched excerpt: use codeterm sessions get".into(),
        },
    }
}

fn sessions_cfg(sessions: usize) -> DecisionRerankConfig {
    let mut config = enabled_cfg();
    config.sources.sessions = sessions;
    config
}

fn request(query: &str, limit: usize) -> SearchParams {
    SearchParams {
        q: query.into(),
        limit: Some(limit),
        decision_rerank: Some(true),
        ..SearchParams::default()
    }
}

#[test]
fn session_candidate_can_win_without_touching_mem_access_counts() {
    let mut backend = seeded();
    backend.sessions.push(session_candidate());
    let before = access_counts(&backend);
    let registry = relevance_registry(&[("Release command", 0.99)]);

    let output = resolve_search(
        &backend,
        &registry,
        &sessions_cfg(1),
        request("codeterm sessions get", 1),
    )
    .unwrap();

    assert_eq!(
        output["hits"],
        json!([{
            "source": "session",
            "session_id": "sess-42",
            "provider": "claude",
            "title": "Release command",
            "excerpt": "Use codeterm sessions get sess-42 to inspect the prior run.",
            "pointer": "codeterm sessions get sess-42",
            "relevance": 0.99,
            "via": "sessions",
        }])
    );
    assert_eq!(
        output["decision_rerank"]["sources"],
        json!({ "recall": TITLES.len(), "graph": 0, "docs": 0, "sessions": 1 })
    );
    assert_eq!(access_counts(&backend), before);
}

#[test]
fn sessions_source_does_not_change_disabled_decision_output() {
    let mut backend = seeded();
    backend.sessions.push(session_candidate());
    let mut query = request("release command", 2);
    query.decision_rerank = Some(false);
    let expected = baseline_oracle(&backend, &query);
    let mut config = DecisionRerankConfig::default();
    config.sources.sessions = 1;

    let output = resolve_search(&backend, &relevance_registry(&[]), &config, query).unwrap();

    assert_eq!(normalized(output.clone()), normalized(expected));
    assert!(output["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["source"] != "session"));
}

#[test]
fn sessions_zero_preserves_the_previous_applied_output() {
    let mut with_candidate = seeded();
    with_candidate.sessions.push(session_candidate());
    let without_candidate = seeded();
    let query = request("release command", 2);
    let registry = relevance_registry(&[("Release command", 0.99), (TITLES[0], 0.2)]);

    let actual =
        resolve_search(&with_candidate, &registry, &sessions_cfg(0), query.clone()).unwrap();
    let expected = resolve_search(&without_candidate, &registry, &enabled_cfg(), query).unwrap();

    assert_eq!(normalized(actual.clone()), normalized(expected));
    assert!(actual["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["source"] != "session"));
}

#[test]
fn fallback_omits_session_candidates_and_matches_baseline() {
    let mut backend = seeded();
    backend.sessions.push(session_candidate());
    let query = request("release command", 2);
    let baseline = baseline_oracle(&backend, &query);

    let output = resolve_search(
        &backend,
        &registry_with(StubMode::Fail(DecisionError::Unavailable(
            "no token".into(),
        ))),
        &sessions_cfg(1),
        query,
    )
    .unwrap();

    assert_eq!(
        output["decision_rerank"],
        json!({ "status": "fallback", "reason": "unavailable" })
    );
    assert_eq!(
        normalized(without_envelope(output.clone())),
        normalized(baseline)
    );
    assert!(output["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["source"] != "session"));
}
