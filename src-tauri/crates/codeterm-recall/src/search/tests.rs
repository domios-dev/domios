use super::*;

fn hit(source: &str, score: f64, id: &str) -> serde_json::Value {
    serde_json::json!({ "source": source, "score": score, "id": id })
}

#[test]
fn merge_keeps_project_plan_hit_when_mem_fills_limit() {
    let mut mem_hits = vec![
        hit("mem", 0.01, "m1"),
        hit("mem", 0.02, "m2"),
        hit("mem", 0.03, "m3"),
    ];
    let plan_hit = codeterm_plan::service_plan::search_index::PlanSearchHit {
        source: "plan",
        score: 0.50,
        id: "p1".into(),
        kind: "task".into(),
        title: "Plan task".into(),
        body: "plan button soft-lock".into(),
        scope: "plan_scope".into(),
        updated_at: 1234,
        age_ms: 5678,
        plan_id: Some("pl_1".into()),
        spec_id: None,
        unit_id: "task_1".into(),
        unit_title: "Task".into(),
        parent_title: "Plan".into(),
        status: Some("pending".into()),
    };

    merge_plan_hits(&mut mem_hits, vec![plan_hit], Some(3));

    assert_eq!(mem_hits.len(), 3);
    assert!(mem_hits.iter().any(is_plan_hit));
}

// Pre-fix value for limit<=50: min(limit*5, 50), and always >= limit.
fn legacy_small_limit(limit: usize) -> usize {
    (limit * 5).min(PLAN_INDEX_LIMIT_CAP)
}

#[test]
fn large_limit_is_bounded_and_never_panics() {
    for limit in [51usize, 60, 100] {
        let eff = effective_plan_index_limit(limit);
        assert!(eff <= PLAN_INDEX_LIMIT_CAP, "limit={limit} eff={eff} > cap");
        assert_eq!(eff, PLAN_INDEX_LIMIT_CAP, "limit>50 must saturate to cap");
    }
}

#[test]
fn small_limit_matches_pre_fix_value() {
    for limit in [1usize, 10, 50] {
        let eff = effective_plan_index_limit(limit);
        assert_eq!(
            eff,
            legacy_small_limit(limit),
            "limit={limit} changed behavior"
        );
        assert!(
            eff >= limit,
            "limit={limit} eff={eff} dropped below user limit"
        );
    }
}

#[test]
fn effective_limit_invariant_over_range() {
    for limit in 1..=1000usize {
        let eff = effective_plan_index_limit(limit);
        let lower = limit.min(PLAN_INDEX_LIMIT_CAP);
        assert!(
            (lower..=PLAN_INDEX_LIMIT_CAP).contains(&eff),
            "limit={limit} eff={eff} out of [{lower}..={PLAN_INDEX_LIMIT_CAP}]"
        );
    }
}

#[test]
fn empty_search_query_is_rejected() {
    assert_eq!(
        validate_search_query(" ").unwrap_err(),
        "search query must not be empty"
    );
}
