use super::*;
use crate::config::schema::SupporterSources;
use crate::mem::Relation;

const ENTITY: &str = "tech_lighthouse";
/// Graph-only memories share no word with `QUERY`; only the entity link reaches them.
const GRAPH_TITLES: [&str; 3] = ["harbor ledger", "tide almanac", "mooring roster"];

/// The fixture store plus graph-only items and one recall item linked to `ENTITY`.
fn graph_seeded() -> SeededBackend {
    let mut backend = seeded();
    let mut graph_ids = Vec::new();
    for title in GRAPH_TITLES {
        let id = backend
            .mem
            .insert_raw(&NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: title.to_string(),
                summary: None,
                area: None,
                tags: Vec::new(),
                body: format!("{title} maritime notes"),
                source: None,
                importance: 0.5,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        graph_ids.push(id);
    }
    let recall_linked = backend.ids[0].clone();
    backend.mem.with_conn(|conn| {
        // Raw insert: `graph_create_entity` also embeds, which would spawn the embedder.
        conn.execute(
            "INSERT INTO entities (id, entity_type, scope, properties_json, created_at, updated_at)
             VALUES (?1, 'Tech', 'global', '{}', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
            [ENTITY],
        )
        .unwrap();
        for from in graph_ids.iter().chain([&recall_linked]) {
            crate::mem::graph::insert_relation(
                conn,
                &Relation {
                    from_id: from.clone(),
                    relation_type: "mentions".into(),
                    to_id: ENTITY.into(),
                    scope: "global".into(),
                    valid_from: "2026-01-01T00:00:00Z".into(),
                    valid_to: None,
                    confidence: 1.0,
                    properties: json!({}),
                    reason: None,
                },
            )
            .unwrap();
        }
    });
    backend.ids.extend(graph_ids);
    backend
}

fn graph_cfg(graph: usize) -> SupporterConfig {
    SupporterConfig {
        memory_rerank: MemoryRerankConfig {
            sources: SupporterSources {
            graph,
                ..enabled_cfg().memory_rerank.sources
            },
            ..enabled_cfg().memory_rerank
        },
        ..enabled_cfg()
    }
}

fn applied(limit: usize) -> SearchParams {
    SearchParams {
        supporter: Some(true),
        ..params(limit)
    }
}

fn hits(output: &Value) -> &Vec<Value> {
    output["hits"].as_array().unwrap()
}

fn via_count(output: &Value, via: &str) -> usize {
    hits(output).iter().filter(|hit| hit["via"] == via).count()
}

fn is_graph_title(title: &str) -> bool {
    GRAPH_TITLES.contains(&title)
}

#[test]
fn decision_graph_only_item_can_win() {
    let backend = graph_seeded();
    let pool_titles = titles(&baseline_oracle(&backend, &params(10)));
    assert_eq!(pool_titles.len(), TITLES.len());
    assert!(!pool_titles.iter().any(|t| is_graph_title(t)));
    let registry = relevance_registry(&[
        (GRAPH_TITLES[0], 0.95),
        (TITLES[0], 0.4),
        (TITLES[1], 0.3),
        (TITLES[2], 0.2),
        (TITLES[3], 0.1),
    ]);

    let output = resolve_search(&backend, &registry, &graph_cfg(3), applied(2)).unwrap();

    let returned = hits(&output);
    assert_eq!(titles(&output), vec![GRAPH_TITLES[0], TITLES[0]]);
    assert_eq!(returned[0]["via"], "graph");
    assert_eq!(returned[0]["source"], "mem");
    assert_eq!(returned[1]["via"], "recall");
    let keys = |hit: &Value| {
        let mut keys: Vec<String> = hit["item"].as_object().unwrap().keys().cloned().collect();
        keys.sort();
        (keys, hit.as_object().unwrap().len())
    };
    assert_eq!(keys(&returned[0]), keys(&returned[1]));
    assert_eq!(output["supporter"]["status"], "applied");
    assert_eq!(
        output["supporter"]["sources"],
        json!({ "recall": TITLES.len(), "graph": GRAPH_TITLES.len() })
    );
}

#[test]
fn decision_graph_counts_and_dedup_respected() {
    let backend = graph_seeded();
    let registry = relevance_registry(&[]);
    for graph in [1, 2, 3, 20] {
        let output = resolve_search(&backend, &registry, &graph_cfg(graph), applied(50)).unwrap();

        let expected = graph.min(GRAPH_TITLES.len());
        let ids: Vec<&str> = hits(&output)
            .iter()
            .map(|hit| hit["item"]["id"].as_str().unwrap())
            .collect();
        let unique: HashSet<&str> = ids.iter().copied().collect();
        assert_eq!(unique.len(), ids.len(), "graph={graph}: duplicate hit");
        assert_eq!(ids.len(), TITLES.len() + expected, "graph={graph}");
        assert_eq!(via_count(&output, "graph"), expected, "graph={graph}");
        assert_eq!(via_count(&output, "recall"), TITLES.len(), "graph={graph}");
        assert!(hits(&output)
            .iter()
            .filter(|hit| hit["via"] == "graph")
            .all(|hit| is_graph_title(&hit_title(hit))));
        assert_eq!(
            output["supporter"]["sources"],
            json!({ "recall": TITLES.len(), "graph": expected }),
            "graph={graph}"
        );
    }
}

#[test]
fn decision_graph_zero_keeps_applied_output_identical() {
    let backend = graph_seeded();
    let cfg = enabled_cfg();
    assert_eq!(cfg.memory_rerank.sources.graph, 0);
    let limit = 3;
    let pooled = baseline_oracle(&backend, &params(cfg.memory_rerank.sources.recall));
    let pool_titles = titles(&pooled);
    let n = pool_titles.len() as f64;
    let mut by_title: Vec<(&str, f64)> = pool_titles
        .iter()
        .enumerate()
        .map(|(i, t)| (t.as_str(), (i as f64 + 1.0) / (n + 1.0)))
        .collect();
    by_title.push((GRAPH_TITLES[0], 0.99));
    let registry = relevance_registry(&by_title);

    let mut expected_hits: Vec<Value> = hits(&pooled).clone();
    expected_hits.reverse();
    expected_hits.truncate(limit);
    for hit in &mut expected_hits {
        let score = by_title
            .iter()
            .find(|(t, _)| *t == hit_title(hit))
            .unwrap()
            .1;
        hit["relevance"] = json!(score);
    }
    let expected = json!({
        "hits": expected_hits,
        "supporter": {
            "status": "applied",
            "adapter": STUB_ID,
            "low_confidence": false,
            "max_relevance": n / (n + 1.0),
        },
    });

    let output = resolve_search(&backend, &registry, &cfg, applied(limit)).unwrap();

    assert_eq!(normalized(output), normalized(expected));
    assert_eq!(backend.graph_calls.load(Ordering::SeqCst), 0);
}

#[test]
fn decision_graph_fallback_has_no_graph_additions() {
    let backend = graph_seeded();
    let mut request = params(2);
    request.project = Some("/seeded/project".into());
    let cases: Vec<(&str, SupporterRegistry)> = vec![
        (
            "adapter_error",
            registry_with(StubMode::Fail(DecisionError::Http(500))),
        ),
        (
            "deadline",
            registry_with(StubMode::Fail(DecisionError::Deadline)),
        ),
        ("no_adapter", SupporterRegistry::new()),
        (
            "unavailable",
            registry_with(StubMode::Fail(DecisionError::Unavailable(
                "no token".into(),
            ))),
        ),
    ];
    for (reason, registry) in cases {
        let baseline = baseline_oracle(&backend, &request);
        let output = resolve_search(
            &backend,
            &registry,
            &graph_cfg(3),
            SearchParams {
                supporter: Some(true),
                ..request.clone()
            },
        )
        .unwrap();
        assert_eq!(
            output["supporter"],
            json!({ "status": "fallback", "reason": reason })
        );
        assert!(
            !titles(&output).iter().any(|t| is_graph_title(t)),
            "{reason}"
        );
        assert_eq!(
            normalized(without_envelope(output)),
            normalized(baseline),
            "{reason}"
        );
    }

    let off = SearchParams {
        supporter: Some(false),
        ..request.clone()
    };
    let calls = backend.graph_calls.load(Ordering::SeqCst);
    let baseline = baseline_oracle(&backend, &off);
    let output =
        resolve_search(&backend, &registry_with_graph_winner(), &graph_cfg(3), off).unwrap();
    assert_eq!(normalized(output), normalized(baseline));
    assert_eq!(backend.graph_calls.load(Ordering::SeqCst), calls);
}

fn registry_with_graph_winner() -> SupporterRegistry {
    relevance_registry(&[(GRAPH_TITLES[0], 0.99)])
}

#[test]
fn decision_graph_access_delta_only_for_returned() {
    let backend = graph_seeded();
    let registry = relevance_registry(&[
        (GRAPH_TITLES[1], 0.9),
        (TITLES[2], 0.8),
        (GRAPH_TITLES[0], 0.3),
        (TITLES[0], 0.2),
    ]);
    let before = access_counts(&backend);

    let output = resolve_search(&backend, &registry, &graph_cfg(3), applied(2)).unwrap();

    let after = access_counts(&backend);
    assert_eq!(titles(&output), vec![GRAPH_TITLES[1], TITLES[2]]);
    assert_eq!(
        output["supporter"]["sources"]["graph"],
        GRAPH_TITLES.len()
    );
    let returned = returned_mem_ids(&output);
    assert_eq!(returned.len(), 2);
    for id in &backend.ids {
        let expected = i64::from(returned.contains(id));
        assert_eq!(after[id] - before[id], expected, "{id}");
    }
}
