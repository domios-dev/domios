use super::*;
use crate::config::schema::DecisionSources;

fn docs_cfg(docs: usize) -> DecisionRerankConfig {
    DecisionRerankConfig {
        sources: DecisionSources {
            docs,
            ..DecisionSources::default()
        },
        ..enabled_cfg()
    }
}

fn applied_query(query: &str, limit: usize) -> SearchParams {
    SearchParams {
        q: query.into(),
        decision_rerank: Some(true),
        limit: Some(limit),
        ..SearchParams::default()
    }
}

#[test]
fn doc_only_candidate_can_win_without_changing_mem_access_counts() {
    let backend = seeded();
    let query = "Bitwarden vault master password auto unlock";
    let section = crate::mem::docs_source::search(query, 1)
        .into_iter()
        .next()
        .expect("doc source match");
    let title = format!("{} § {}", section.doc_name, section.section_title);
    let registry = relevance_registry(&[(&title, 0.99)]);
    let before = access_counts(&backend);

    let output = resolve_search(
        &backend,
        &registry,
        &docs_cfg(1),
        applied_query(query, 1),
    )
    .unwrap();

    assert_eq!(
        output["hits"],
        json!([{
            "source": "doc",
            "doc": section.doc_name,
            "section": section.section_title,
            "anchor": section.anchor,
            "pointer": format!("codeterm docs {}", section.doc_name),
            "relevance": 0.99,
            "via": "docs",
        }])
    );
    assert_eq!(
        output["decision_rerank"]["sources"],
        json!({ "recall": 0, "graph": 0, "docs": 1 })
    );
    assert_eq!(access_counts(&backend), before);
}

#[test]
fn docs_zero_keeps_the_existing_applied_output() {
    let backend = seeded();
    let baseline = baseline_oracle(&backend, &params(10));
    let scores = [
        (TITLES[0], 0.9),
        (TITLES[1], 0.8),
        (TITLES[2], 0.7),
        (TITLES[3], 0.6),
    ];
    let registry = relevance_registry(&scores);
    let mut expected_hits = baseline["hits"].as_array().unwrap().clone();
    expected_hits.sort_by(|a, b| {
        let score = |hit: &Value| {
            let title = hit_title(hit);
            scores
                .iter()
                .find(|(candidate, _)| *candidate == title.as_str())
                .map(|(_, score)| *score)
                .unwrap_or(0.0)
        };
        score(b).total_cmp(&score(a))
    });
    expected_hits.truncate(2);
    for hit in &mut expected_hits {
        let title = hit_title(hit);
        let relevance = scores
            .iter()
            .find(|(candidate, _)| *candidate == title.as_str())
            .unwrap()
            .1;
        hit["relevance"] = json!(relevance);
    }
    let expected = json!({
        "hits": expected_hits,
        "decision_rerank": {
            "status": "applied",
            "adapter": STUB_ID,
            "low_confidence": false,
            "max_relevance": 0.9,
        },
    });

    let output = resolve_search(
        &backend,
        &registry,
        &docs_cfg(0),
        applied_query(QUERY, 2),
    )
    .unwrap();

    assert_eq!(normalized(output), normalized(expected));
}

#[test]
fn docs_are_absent_from_off_and_fallback_results() {
    let backend = seeded();
    let query = "Bitwarden vault master password auto unlock";
    let plain = SearchParams {
        q: query.into(),
        limit: Some(2),
        decision_rerank: Some(false),
        ..SearchParams::default()
    };
    let baseline = baseline_oracle(&backend, &plain);
    let off = resolve_search(
        &backend,
        &relevance_registry(&[]),
        &docs_cfg(3),
        plain,
    )
    .unwrap();
    assert_eq!(normalized(off), normalized(baseline));

    let request = applied_query(query, 2);
    let baseline = baseline_oracle(&backend, &request);
    let failed = resolve_search(
        &backend,
        &registry_with(StubMode::Fail(DecisionError::Unavailable("no token".into()))),
        &docs_cfg(3),
        request,
    )
    .unwrap();
    assert_eq!(
        failed["decision_rerank"],
        json!({ "status": "fallback", "reason": "unavailable" })
    );
    assert_eq!(
        normalized(without_envelope(failed.clone())),
        normalized(baseline)
    );
    assert!(failed["hits"]
        .as_array()
        .unwrap()
        .iter()
        .all(|hit| hit["source"] != "doc"));
}
