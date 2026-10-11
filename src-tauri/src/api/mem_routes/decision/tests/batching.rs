use super::*;

#[test]
fn search_batches_ten_candidates_and_repeated_searches_each_make_one_reflex() {
    let mut backend = seeded();
    for i in 4..33 {
        let id = backend
            .mem
            .insert_raw(&NewMemItem {
                kind: MemKind::Fact,
                scope: MemScope::Global,
                title: format!("lighthouse {i}"),
                summary: None,
                area: None,
                tags: Vec::new(),
                body: "lighthouse keeper log entry".into(),
                source: None,
                importance: 0.5,
                source_path: None,
                pinned: false,
            })
            .unwrap();
        backend.ids.push(id);
    }
    let mut config = enabled_cfg();
    config.memory_rerank.candidates = 33;
    config.memory_rerank.sources.recall = 33;
    let registry = relevance_registry(&[]);
    for expected in 1..=2 {
        let output = resolve_search(
            &backend,
            &registry,
            &config,
            SearchParams {
                supporter: Some(true),
                ..params(2)
            },
        )
        .unwrap();
        assert_eq!(output["supporter"]["status"], "applied");
        assert_eq!(backend.runtime.status(0)["calls_last_hour"], expected);
        assert_eq!(
            backend.runtime.feed(100, None).unwrap().len(),
            expected as usize
        );
    }
    let journal =
        std::fs::read_to_string(backend._journal_dir.path().join("journal.jsonl")).unwrap();
    let verdicts: Vec<Value> = journal
        .lines()
        .filter_map(|line| serde_json::from_str::<Value>(line).ok())
        .filter(|row| row["kind"] == "rerank")
        .collect();
    assert_eq!(verdicts.len(), 2);
    assert!(verdicts
        .iter()
        .all(|row| row["question_count"] == 10
            && row["distribution"].as_object().unwrap().len() == 10));
}

#[test]
fn three_candidates_preserve_first_stage_and_make_no_model_call() {
    let backend = seeded();
    let mut config = enabled_cfg();
    config.memory_rerank.sources.recall = 3;
    config.memory_rerank.candidates = 3;
    let output = resolve_search(
        &backend,
        &registry_with(StubMode::Fail(DecisionError::Unavailable(
            "must not call".into(),
        ))),
        &config,
        SearchParams {
            supporter: Some(true),
            ..params(3)
        },
    )
    .unwrap();
    assert_eq!(output["supporter"]["status"], "skipped");
    assert_eq!(output["supporter"]["reason"], "small_pool");
    assert_eq!(
        titles(&output),
        titles(&baseline_oracle(&backend, &params(3)))
    );
    assert_eq!(backend.runtime.status(0)["calls_last_hour"], 0);
    assert!(backend.runtime.feed(100, None).unwrap().is_empty());
}
