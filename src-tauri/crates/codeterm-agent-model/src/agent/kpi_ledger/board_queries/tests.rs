use super::*;
use crate::agent::kpi_ledger::{KpiEventInput, KpiEventKind};

#[test]
fn board_queries_group_tokens_and_bound_the_range() {
    let root = tempfile::tempdir().unwrap();
    let ledger = KpiLedger::open(root.path()).unwrap();
    for (index, timestamp) in [86400, 86401, 172800, 259200].into_iter().enumerate() {
        ledger
            .insert(&KpiEventInput {
                plan_id: Some("plan-a".into()),
                task_id: None,
                tab_id: "tab".into(),
                role: None,
                provider: "provider".into(),
                model: Some("model".into()),
                timestamp,
                kind: KpiEventKind::TokenUsage,
                bytes: 15,
                tokens: Some(TokenCounts {
                    input: 10,
                    uncached_input: 4,
                    output: 5,
                }),
                dedup_key: Some(format!("row-{index}")),
                workspace_id: None,
                tab_kind: None,
            })
            .unwrap();
    }
    let daily = ledger.daily_token_usage(86400, 259200).unwrap();
    assert_eq!(daily.len(), 2);
    assert_eq!(daily[0].timestamp, 86400);
    assert_eq!(daily[0].turns, 2);
    assert_eq!(
        daily[0].tokens,
        TokenCounts {
            input: 20,
            uncached_input: 8,
            output: 10
        }
    );
    assert_eq!(daily[1].turns, 1);
    assert_eq!(ledger.daily_token_usage(86401, 172800).unwrap()[0].turns, 1);
    let plan = ledger.aggregate_for_plan("plan-a").unwrap().unwrap();
    assert_eq!(plan.turns, 4);
    assert_eq!(plan.input_tokens, 40);
    assert_eq!(ledger.aggregate_for_plan("absent").unwrap(), None);
}

#[test]
fn board_queries_use_covering_indexes() {
    let root = tempfile::tempdir().unwrap();
    let ledger = KpiLedger::open(root.path()).unwrap();
    let conn = ledger.conn.lock();
    for (sql, expected) in [
        ("SELECT SUM(input_tokens) FROM kpi_events WHERE plan_id = 'p' AND kind != 'tab_created'", "idx_kpi_plan_totals"),
        ("SELECT provider, model, SUM(input_tokens) FROM kpi_events WHERE kind = 'token_usage' GROUP BY provider, model", "idx_kpi_model_totals"),
        ("SELECT ts / 86400, plan_id, provider, model, SUM(input_tokens) FROM kpi_events WHERE kind = 'token_usage' AND ts / 86400 >= 1 AND ts / 86400 <= 2 AND ts >= 86400 AND ts < 259200 GROUP BY ts / 86400, plan_id, provider, model", "idx_kpi_token_days"),
    ] {
        let mut stmt = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
        let details = stmt.query_map([], |row| row.get::<_, String>(3)).unwrap().collect::<Result<Vec<_>, _>>().unwrap().join(" ");
        assert!(details.contains(expected), "{details}");
        assert!(!details.contains("TEMP B-TREE"), "{details}");
    }
}
