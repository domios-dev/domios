use super::*;

use codeterm_service_core::agent::kpi_ledger::TokenCounts;

fn range(from: &str, to: &str) -> SpendRange {
    SpendRange::from_query(&TeamSpendQuery {
        metric: Some("tokens".into()),
        model: Some("all".into()),
        from: Some(from.into()),
        to: Some(to.into()),
    })
    .unwrap()
}

fn plan(id: &str, title: &str, team_id: Option<&str>, status: PlanStatus) -> SpendPlan {
    SpendPlan {
        id: id.into(),
        title: title.into(),
        team_id: team_id.map(str::to_string),
        status,
    }
}

fn event(
    plan_id: Option<&str>,
    provider: &str,
    model: Option<&str>,
    day: &str,
    input: u64,
    uncached_input: u64,
    output: u64,
) -> DailyTokenUsage {
    let timestamp = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp() as u64;
    DailyTokenUsage {
        plan_id: plan_id.map(str::to_string),
        provider: provider.into(),
        model: model.map(str::to_string),
        timestamp,
        tokens: TokenCounts {
            input,
            uncached_input,
            output,
        },
        turns: 1,
    }
}

fn usage_row(
    provider: &str,
    account_id: Option<&str>,
    day: &str,
    weekly_pct: Option<f32>,
) -> UsageSnapshotRow {
    let captured_at = NaiveDate::parse_from_str(day, "%Y-%m-%d")
        .unwrap()
        .and_hms_opt(12, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis();
    UsageSnapshotRow {
        billing_mode: Default::default(),
        limits_reported: false,
        id: 1,
        provider: provider.into(),
        account_id: account_id.map(str::to_string),
        captured_at,
        parsed_ok: true,
        session_pct: None,
        session_resets_at: None,
        weekly_pct,
        weekly_resets_at: Some(2_000),
        spend_cents: None,
        spend_limit_cents: None,
        spend_remaining_cents: None,
        spend_resets_at: None,
        failure_reason: None,
        probe_command: None,
        raw_excerpt: None,
        debug_json: None,
    }
}

#[test]
fn team_spend_rejects_malformed_or_unbounded_inputs() {
    let malformed = TeamSpendQuery {
        metric: Some("invalid".into()),
        model: Some("all".into()),
        from: Some("2026-01-01".into()),
        to: Some("2026-01-01".into()),
    };
    assert_eq!(
        parse_request("team-a", &malformed).unwrap_err(),
        "unsupported metric: invalid"
    );
    assert!(parse_request(
        "team-a",
        &TeamSpendQuery {
            metric: Some("tokens".into()),
            model: Some("all".into()),
            from: Some("2026-02-01".into()),
            to: Some("2026-01-01".into()),
        }
    )
    .unwrap_err()
    .contains("earlier"));
    assert!(parse_request(
        "team-a",
        &TeamSpendQuery {
            metric: Some("tokens".into()),
            model: Some("all".into()),
            from: Some("2026-01-01".into()),
            to: Some("2026-02-02".into()),
        }
    )
    .unwrap_err()
    .contains("31 days"));
    assert!(parse_request(
        " team-a",
        &TeamSpendQuery {
            metric: Some("tokens".into()),
            model: Some("all".into()),
            from: Some("2026-01-01".into()),
            to: Some("2026-01-01".into()),
        }
    )
    .unwrap_err()
    .contains("whitespace"));
}

#[test]
fn team_spend_aggregates_active_archived_and_excludes_unknown_rows_without_foreign_data() {
    let plans = vec![
        plan("pl-active", "Active", Some("team-a"), PlanStatus::Active),
        plan(
            "pl-archived",
            "Archived",
            Some("team-a"),
            PlanStatus::Archived,
        ),
        plan("pl-foreign", "Foreign", Some("team-b"), PlanStatus::Active),
    ];
    let events = vec![
        event(
            Some("pl-active"),
            "codex",
            Some("gpt"),
            "2026-01-01",
            100,
            60,
            20,
        ),
        event(
            Some("pl-active"),
            "codex",
            Some("gpt"),
            "2026-01-01",
            25,
            5,
            10,
        ),
        event(Some("pl-active"), "", None, "2026-01-01", 30, 10, 5),
        event(
            Some("pl-archived"),
            "claude",
            Some("opus"),
            "2026-01-02",
            200,
            100,
            40,
        ),
        event(
            Some("pl-foreign"),
            "codex",
            Some("gpt"),
            "2026-01-01",
            900,
            900,
            900,
        ),
        event(
            Some("pl-missing"),
            "codex",
            Some("gpt"),
            "2026-01-01",
            9,
            4,
            1,
        ),
        event(None, "", None, "2026-01-01", 7, 3, 2),
    ];
    let projection = project_tokens(
        &plans,
        &events,
        "team-a",
        range("2026-01-01", "2026-01-02"),
        ModelSelector::All,
    );
    assert_eq!(projection.attribution.team_plan_count, 2);
    assert_eq!(projection.attribution.foreign_plan_rows_excluded, 1);
    assert_eq!(projection.attribution.unknown_plan_rows, 1);
    assert_eq!(projection.attribution.unassigned_plan_rows, 1);
    assert_eq!(projection.daily.len(), 3);
    assert!(projection.daily.iter().all(|row| matches!(
        row.plan_id.as_deref(),
        Some("pl-active") | Some("pl-archived")
    )));
    let archived = projection
        .daily
        .iter()
        .find(|row| row.plan_id.as_deref() == Some("pl-archived"))
        .unwrap();
    assert_eq!(archived.plan_status, Some(PlanStatus::Archived));
    assert_eq!(archived.output_tokens, 40);
    let active = projection
        .daily
        .iter()
        .find(|row| {
            row.plan_id.as_deref() == Some("pl-active") && row.model.as_deref() == Some("gpt")
        })
        .unwrap();
    assert_eq!(active.input_tokens, 125);
    assert_eq!(active.uncached_input_tokens, 65);
    assert_eq!(active.output_tokens, 30);
    assert_eq!(active.turns, 2);
    assert!(projection
        .daily
        .iter()
        .all(|row| row.plan_id.as_deref() != Some("pl-missing")));
}

#[test]
fn team_spend_excludes_planless_rows_and_counts_unassigned_attribution() {
    let plans = vec![plan(
        "pl-active",
        "Active",
        Some("team-a"),
        PlanStatus::Active,
    )];
    let events = vec![event(None, "", None, "2026-01-01", 7, 3, 2)];
    let projection = project_tokens(
        &plans,
        &events,
        "team-a",
        range("2026-01-01", "2026-01-01"),
        ModelSelector::All,
    );
    assert!(projection.daily.is_empty());
    assert_eq!(projection.attribution.unassigned_plan_rows, 1);
    assert_eq!(projection.attribution.unknown_plan_rows, 0);
    assert_eq!(projection.attribution.foreign_plan_rows_excluded, 0);
}

#[test]
fn team_spend_model_filters_keep_all_unknowns_only_in_all_selector() {
    let plans = vec![plan(
        "pl-active",
        "Active",
        Some("team-a"),
        PlanStatus::Active,
    )];
    let events = vec![
        event(
            Some("pl-active"),
            "codex",
            Some("gpt"),
            "2026-01-01",
            100,
            50,
            20,
        ),
        event(
            Some("pl-active"),
            "codex",
            Some("other"),
            "2026-01-01",
            200,
            100,
            40,
        ),
        event(Some("pl-active"), "codex", None, "2026-01-01", 300, 150, 60),
    ];
    let all = project_tokens(
        &plans,
        &events,
        "team-a",
        range("2026-01-01", "2026-01-01"),
        ModelSelector::All,
    );
    assert_eq!(all.daily.len(), 3);
    let selected = project_tokens(
        &plans,
        &events,
        "team-a",
        range("2026-01-01", "2026-01-01"),
        ModelSelector::One("gpt"),
    );
    assert_eq!(selected.daily.len(), 1);
    assert_eq!(selected.daily[0].model.as_deref(), Some("gpt"));
    assert_eq!(selected.daily[0].input_tokens, 100);
}

#[test]
fn team_spend_date_bounds_exclude_outside_days() {
    let plans = vec![plan(
        "pl-active",
        "Active",
        Some("team-a"),
        PlanStatus::Active,
    )];
    let events = vec![
        event(
            Some("pl-active"),
            "codex",
            Some("gpt"),
            "2026-01-01",
            1,
            1,
            1,
        ),
        event(
            Some("pl-active"),
            "codex",
            Some("gpt"),
            "2026-01-02",
            2,
            2,
            2,
        ),
        event(
            Some("pl-active"),
            "codex",
            Some("gpt"),
            "2026-01-03",
            4,
            4,
            4,
        ),
    ];
    let projection = project_tokens(
        &plans,
        &events,
        "team-a",
        range("2026-01-02", "2026-01-02"),
        ModelSelector::All,
    );
    assert_eq!(projection.daily.len(), 1);
    assert_eq!(projection.daily[0].day, "2026-01-02");
}

#[test]
fn team_spend_weekly_points_preserve_provider_account_and_machine_scope() {
    let rows = vec![
        usage_row("codex", Some("account-a"), "2026-01-02", Some(35.5)),
        usage_row("codex", Some("account-a"), "2026-01-01", Some(25.0)),
        usage_row("claude", None, "2026-01-01", Some(80.0)),
        usage_row("claude", None, "2026-01-03", Some(90.0)),
    ];
    let points = weekly_limit_points_from_rows(rows, range("2026-01-01", "2026-01-02"));
    assert_eq!(points.len(), 2);
    let account = points
        .iter()
        .find(|point| point.provider == "codex")
        .unwrap();
    assert_eq!(account.weekly_limit_points, 35.5);
    assert_eq!(account.scope.kind, "provider_account");
    assert_eq!(account.scope.account_id.as_deref(), Some("account-a"));
    assert_eq!(account.team_attribution, "unavailable");
    assert_eq!(account.model_attribution, "unavailable");
    let machine = points
        .iter()
        .find(|point| point.provider == "claude")
        .unwrap();
    assert_eq!(machine.scope.kind, "machine");
    assert_eq!(machine.scope.account_id, None);
}

#[test]
fn team_spend_weekly_days_count_points_used_per_day_across_a_reset() {
    let mut later = usage_row("codex", Some("account-a"), "2026-01-03", Some(45.0));
    later.captured_at += 3_600_000;
    let mut reset = usage_row("codex", Some("account-a"), "2026-01-04", Some(5.0));
    reset.weekly_resets_at = Some(2_000 + 7 * 86_400_000);
    let rows = vec![
        usage_row("codex", Some("account-a"), "2026-01-01", Some(10.0)),
        usage_row("codex", Some("account-a"), "2026-01-02", Some(25.0)),
        usage_row("codex", Some("account-a"), "2026-01-03", Some(40.0)),
        later,
        reset,
        usage_row("claude", None, "2026-01-03", Some(80.0)),
    ];
    let days = weekly_limit_days_from_rows(&rows, range("2026-01-02", "2026-01-04"));
    let codex: Vec<_> = days
        .iter()
        .map(|day| (day.day.as_str(), day.points_used, day.weekly_limit_points))
        .collect();
    assert_eq!(
        codex,
        vec![
            ("2026-01-02", 15.0, 25.0),
            ("2026-01-03", 20.0, 45.0),
            ("2026-01-04", 5.0, 5.0),
        ],
        "a first reading has no baseline, so claude has no day"
    );
    assert!(days.iter().all(|day| day.scope.kind == "provider_account"
        && day.team_attribution == "unavailable"
        && day.model_attribution == "unavailable"));
}

#[test]
fn team_spend_weekly_model_filter_is_rejected_without_attribution() {
    let query = TeamSpendQuery {
        metric: Some("weekly_limit_points".into()),
        model: Some("gpt".into()),
        from: Some("2026-01-01".into()),
        to: Some("2026-01-01".into()),
    };
    assert!(parse_request("team-a", &query)
        .unwrap_err()
        .contains("no model attribution"));
}
