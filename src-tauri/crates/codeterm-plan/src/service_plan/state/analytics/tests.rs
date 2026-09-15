use std::sync::Arc;

use chrono::NaiveDate;

use super::*;
use crate::service_plan::state::mission_control::{ProviderAllowance, KPI_ENABLED_SETTING};

const DAY: u64 = 86_400;
const START: u64 = 1_788_220_800;
const NOW: u64 = START + 3 * DAY;

fn day(value: &str) -> NaiveDate {
    NaiveDate::parse_from_str(value, "%Y-%m-%d").unwrap()
}

fn query(group_by: AnalyticsGroupBy) -> AnalyticsQuery {
    AnalyticsQuery {
        range: AnalyticsRange {
            from: "2026-09-01".into(),
            to: "2026-09-03".into(),
            tz: "UTC".into(),
        },
        bucket: AnalyticsBucket::Day,
        filters: AnalyticsFilters::default(),
        group_by,
        scope: None,
    }
}

fn buckets() -> Vec<BucketBound> {
    (0..3u64)
        .map(|index| BucketBound {
            start: START + index * DAY,
            end: START + (index + 1) * DAY,
            label: format!("2026-09-0{}", index + 1),
        })
        .collect()
}

fn directory() -> ScopeDirectory {
    let mut directory = ScopeDirectory::default();
    directory
        .providers
        .insert("prov-a".into(), "Provider A".into());
    directory.workspaces.insert("ws-1".into(), "Alpha".into());
    directory.workspaces.insert("ws-2".into(), "Beta".into());
    directory.teams.insert("team-1".into(), Some("Red".into()));
    directory.plans.insert(
        "pl-team".into(),
        DirectoryPlan {
            title: "Teamed".into(),
            team_id: Some("team-1".into()),
        },
    );
    directory.plans.insert(
        "pl-solo".into(),
        DirectoryPlan {
            title: "Solo".into(),
            team_id: None,
        },
    );
    directory.plans.insert(
        "pl-orphan-team".into(),
        DirectoryPlan {
            title: "Orphan".into(),
            team_id: Some("team-gone".into()),
        },
    );
    directory
}

#[allow(clippy::too_many_arguments)]
fn row(
    bucket: usize,
    provider: &str,
    model: Option<&str>,
    workspace: Option<&str>,
    plan: Option<&str>,
    input: u64,
    uncached: u64,
    output: u64,
) -> TokenAggregateRow {
    TokenAggregateRow {
        bucket,
        provider: provider.into(),
        model: model.map(str::to_string),
        workspace_id: workspace.map(str::to_string),
        plan_id: plan.map(str::to_string),
        turns: 1,
        input_tokens: input,
        uncached_input_tokens: uncached,
        output_tokens: output,
    }
}

fn coverage_from(first: Option<u64>, mark: Option<u64>, tabs: Option<u64>) -> LedgerCoverage {
    LedgerCoverage {
        first_recorded_at: first,
        last_recorded_at: first,
        workspace_recorded_since: mark,
        tabs_counted_since: tabs,
    }
}

fn recorded(
    coverage: LedgerCoverage,
    tokens: Vec<TokenAggregateRow>,
    tabs: Vec<TabCreatedAggregateRow>,
) -> LedgerRead {
    LedgerRead::Recorded(Arc::new(LedgerAggregates {
        coverage,
        tokens,
        tabs,
    }))
}

fn sample_ledger() -> LedgerRead {
    recorded(
        coverage_from(Some(START), Some(START), Some(START + DAY)),
        vec![
            row(
                0,
                "prov-a",
                Some("m1"),
                Some("ws-1"),
                Some("pl-team"),
                30,
                10,
                5,
            ),
            row(1, "prov-a", None, Some("ws-2"), Some("pl-solo"), 20, 20, 1),
            row(2, "prov-a", Some("m1"), None, None, 7, 3, 2),
            row(
                2,
                "prov-b",
                Some("m2"),
                Some("ws-gone"),
                Some("pl-gone"),
                4,
                4,
                4,
            ),
            row(1, "", None, Some("ws-1"), Some("pl-orphan-team"), 1, 1, 1),
        ],
        Vec::new(),
    )
}

fn project_at(
    query: &AnalyticsQuery,
    ledger: &LedgerRead,
    now: u64,
) -> Result<AnalyticsSnapshot, AnalyticsRefusal> {
    let buckets = buckets();
    let directory = directory();
    project_snapshot(&ProjectionInput {
        query,
        buckets: &buckets,
        directory: &directory,
        ledger,
        allowance: &[],
        now,
    })
}

fn project(query: &AnalyticsQuery, ledger: &LedgerRead) -> AnalyticsSnapshot {
    project_at(query, ledger, NOW).expect("an in-scope query projects")
}

fn keys(rows: &[GroupRow]) -> Vec<(Option<&str>, Option<KeyReason>)> {
    rows.iter()
        .map(|row| (row.key.id.as_deref(), row.key.reason))
        .collect()
}

fn refused(edit: impl Fn(&mut AnalyticsQuery)) -> AnalyticsRefusalKind {
    let mut candidate = query(AnalyticsGroupBy::Workspace);
    edit(&mut candidate);
    validate_query(&candidate).unwrap_err().error
}

#[test]
fn analytics_host_validation_refuses_malformed_ranges_filters_and_mesh_scope() {
    assert_eq!(
        validate_query(&query(AnalyticsGroupBy::Workspace)).unwrap(),
        LocalDays {
            from: day("2026-09-01"),
            to: day("2026-09-03"),
        }
    );
    let invalid = AnalyticsRefusalKind::InvalidQuery;
    assert_eq!(refused(|q| q.range.to = "2026-08-31".into()), invalid);
    assert_eq!(refused(|q| q.range.from = "09/01/2026".into()), invalid);
    assert_eq!(
        refused(|q| {
            q.bucket = AnalyticsBucket::Week;
            q.range.from = "2026-01-01".into();
            q.range.to = "2026-04-03".into();
        }),
        invalid
    );
    assert_eq!(
        refused(|q| {
            q.range.from = "2026-01-01".into();
            q.range.to = "2026-02-01".into();
        }),
        invalid,
        "day buckets stop at 31 days"
    );
    assert_eq!(refused(|q| q.range.tz = " ".into()), invalid);
    assert_eq!(
        refused(|q| q.filters.workspaces = Some(vec![Some(" ws-1".into())])),
        invalid
    );
    assert_eq!(
        refused(|q| {
            q.filters.providers = Some((0..=MAX_FILTER_KEYS).map(|i| format!("p{i}")).collect())
        }),
        invalid
    );
    let foreign = AnalyticsRefusalKind::ForeignScope;
    assert_eq!(
        refused(|q| {
            q.scope = Some(AnalyticsScopeRequest {
                kind: MACHINE_SCOPE.into(),
                includes_mesh_peers: true,
            })
        }),
        foreign
    );
    assert_eq!(
        refused(|q| {
            q.scope = Some(AnalyticsScopeRequest {
                kind: "mesh".into(),
                includes_mesh_peers: false,
            })
        }),
        foreign
    );

    let mut widest = query(AnalyticsGroupBy::Plan);
    widest.bucket = AnalyticsBucket::Week;
    widest.range.from = "2026-01-01".into();
    widest.range.to = "2026-04-02".into();
    assert!(
        validate_query(&widest).is_ok(),
        "92 days is the bound itself"
    );

    let range =
        serde_json::json!({ "from": "2026-09-01", "to": "2026-09-03", "tz": "Europe/Riga" });
    let unknown =
        serde_json::json!({ "range": range, "bucket": "day", "group_by": "plan", "limit": 5 });
    assert!(serde_json::from_value::<AnalyticsQuery>(unknown).is_err());
    let parsed: AnalyticsQuery = serde_json::from_value(serde_json::json!({
        "range": range,
        "bucket": "day",
        "group_by": "provider_model",
        "filters": {
            "workspaces": [null, "ws-1"],
            "models": [{ "provider": "prov-a", "model": null }]
        }
    }))
    .unwrap();
    assert_eq!(
        parsed.filters.workspaces,
        Some(vec![None, Some("ws-1".to_string())])
    );
    assert_eq!(
        parsed.filters.models,
        Some(vec![ModelKey {
            provider: "prov-a".into(),
            model: None,
        }])
    );
}

#[test]
fn analytics_host_local_buckets_tile_the_range_without_gaps() {
    let daily = local_buckets(
        LocalDays {
            from: day("2026-09-01"),
            to: day("2026-09-03"),
        },
        AnalyticsBucket::Day,
    );
    assert_eq!(daily.len(), 3);
    assert_eq!(daily[0].first_day, day("2026-09-01"));
    assert_eq!(daily[2].end_day, day("2026-09-04"));
    assert!(daily
        .windows(2)
        .all(|pair| pair[0].end_day == pair[1].first_day));

    let weekly = local_buckets(
        LocalDays {
            from: day("2026-09-01"),
            to: day("2026-09-10"),
        },
        AnalyticsBucket::Week,
    );
    assert_eq!(
        weekly,
        vec![
            LocalBucket {
                first_day: day("2026-09-01"),
                end_day: day("2026-09-08"),
            },
            LocalBucket {
                first_day: day("2026-09-08"),
                end_day: day("2026-09-11"),
            },
        ]
    );
}

#[test]
fn analytics_host_disabled_and_unavailable_ledgers_never_report_zero() {
    let by_model = query(AnalyticsGroupBy::ProviderModel);
    let disabled = project(&by_model, &LedgerRead::Disabled);
    assert_eq!(disabled.recording.state, RecordingState::Disabled);
    assert_eq!(disabled.recording.enable_setting, Some(KPI_ENABLED_SETTING));
    assert_eq!(disabled.tokens.totals, None);
    assert!(disabled
        .tokens
        .series
        .iter()
        .all(|bucket| bucket.coverage == Coverage::NotRecorded && bucket.totals.is_none()));
    assert_eq!(disabled.tabs_created.state, TabsCreatedState::NotCounted);
    assert_eq!(disabled.tabs_created.total, None);
    let json = serde_json::to_value(&disabled).unwrap();
    assert!(json["tokens"]["totals"].is_null());
    assert_eq!(json["money"]["state"], "not_recorded");
    assert_eq!(
        json["scope"],
        serde_json::json!({ "kind": "machine", "includes_mesh_peers": false })
    );

    let reason = "database disk image is malformed";
    let unavailable = project(
        &by_model,
        &LedgerRead::Unavailable {
            reason: reason.into(),
        },
    );
    assert_eq!(unavailable.recording.state, RecordingState::Unavailable);
    assert_eq!(unavailable.recording.reason.as_deref(), Some(reason));
    assert_eq!(
        unavailable.recording.enable_setting, None,
        "turning the setting on does not repair a ledger that will not open"
    );
    assert_eq!(unavailable.tokens.totals, None);
}

#[test]
fn analytics_host_idle_recorded_buckets_are_real_zeros_and_unrecorded_ones_stay_missing() {
    let by_workspace = query(AnalyticsGroupBy::Workspace);
    let idle = recorded(
        coverage_from(None, Some(START), None),
        Vec::new(),
        Vec::new(),
    );
    let snapshot = project(&by_workspace, &idle);
    assert_eq!(snapshot.tokens.totals, Some(Totals::default()));
    assert!(snapshot
        .tokens
        .series
        .iter()
        .all(|bucket| bucket.coverage == Coverage::Recorded
            && bucket.totals == Some(Totals::default())));

    let late = recorded(
        coverage_from(None, Some(START + DAY + 3_600), None),
        Vec::new(),
        Vec::new(),
    );
    let snapshot = project_at(&by_workspace, &late, START + 2 * DAY + 60).unwrap();
    let coverages: Vec<Coverage> = snapshot
        .tokens
        .series
        .iter()
        .map(|bucket| bucket.coverage)
        .collect();
    assert_eq!(
        coverages,
        vec![Coverage::NotRecorded, Coverage::Partial, Coverage::Partial]
    );
    assert_eq!(snapshot.tokens.series[0].totals, None);
    assert_eq!(snapshot.tokens.series[1].totals, Some(Totals::default()));

    let before_now = project_at(&by_workspace, &late, START + DAY + 7_200).unwrap();
    assert_eq!(
        before_now.tokens.series[2].coverage,
        Coverage::NotRecorded,
        "a bucket that has not begun is not recorded"
    );

    let unmarked = recorded(LedgerCoverage::default(), Vec::new(), Vec::new());
    assert_eq!(
        project(&by_workspace, &unmarked).tokens.totals,
        None,
        "a ledger with no recording mark claims no zero"
    );
}

#[test]
fn analytics_host_groups_every_dimension_with_honest_null_keys() {
    let ledger = sample_ledger();
    let expected = Totals {
        new_input: 38,
        generated: 13,
        read_from_cache: 24,
        turns: 5,
    };

    let by_model = project(&query(AnalyticsGroupBy::ProviderModel), &ledger);
    assert_eq!(by_model.tokens.totals, Some(expected));
    assert_eq!(
        keys(&by_model.tokens.groups),
        vec![
            (Some("prov-a"), None),
            (Some("prov-b"), None),
            (None, Some(KeyReason::ProviderNotReported)),
        ]
    );
    let prov_a = &by_model.tokens.groups[0];
    assert_eq!(prov_a.label.as_deref(), Some("Provider A"));
    assert_eq!(
        keys(&prov_a.children),
        vec![
            (None, Some(KeyReason::ModelNotReported)),
            (Some("m1"), None)
        ]
    );
    assert!(prov_a
        .children
        .iter()
        .all(|child| child.key.provider.as_deref() == Some("prov-a")));
    assert_eq!(by_model.tokens.groups[1].label.as_deref(), Some("prov-b"));
    assert_eq!(by_model.tokens.groups[2].label, None);
    let last = &by_model.tokens.series[2];
    assert_eq!(last.totals.map(|totals| totals.new_input), Some(7));
    assert_eq!(
        last.groups
            .iter()
            .map(|group| (group.key.provider.as_deref(), group.key.id.as_deref()))
            .collect::<Vec<_>>(),
        vec![(Some("prov-b"), Some("m2")), (Some("prov-a"), Some("m1"))]
    );

    let by_workspace = project(&query(AnalyticsGroupBy::Workspace), &ledger);
    assert_eq!(
        keys(&by_workspace.tokens.groups),
        vec![
            (Some("ws-2"), None),
            (Some("ws-1"), None),
            (Some("ws-gone"), Some(KeyReason::WorkspaceDeleted)),
            (None, Some(KeyReason::WorkspaceNotRecorded)),
        ]
    );
    assert_eq!(
        by_workspace.tokens.groups[1].label.as_deref(),
        Some("Alpha")
    );
    assert_eq!(by_workspace.tokens.groups[2].label, None);
    assert!(by_workspace
        .tokens
        .groups
        .iter()
        .all(|group| group.children.is_empty()));

    let by_team = project(&query(AnalyticsGroupBy::Team), &ledger);
    assert_eq!(
        keys(&by_team.tokens.groups),
        vec![
            (None, Some(KeyReason::NoTeam)),
            (Some("team-1"), None),
            (None, Some(KeyReason::PlanDeleted)),
            (Some("team-gone"), Some(KeyReason::TeamDeleted)),
        ]
    );
    assert_eq!(by_team.tokens.groups[1].label.as_deref(), Some("Red"));
    assert_eq!(by_team.tokens.groups[3].label, None);

    let by_plan = project(&query(AnalyticsGroupBy::Plan), &ledger);
    assert_eq!(
        keys(&by_plan.tokens.groups),
        vec![
            (Some("pl-solo"), None),
            (Some("pl-team"), None),
            (Some("pl-gone"), Some(KeyReason::PlanDeleted)),
            (None, Some(KeyReason::NoPlan)),
            (Some("pl-orphan-team"), None),
        ]
    );
    assert_eq!(by_plan.tokens.groups[0].label.as_deref(), Some("Solo"));

    for snapshot in [&by_model, &by_workspace, &by_team, &by_plan] {
        let summed = snapshot
            .tokens
            .groups
            .iter()
            .fold(0, |sum, group| sum + group.totals.new_input);
        assert_eq!(
            summed, expected.new_input,
            "every row lands in exactly one group"
        );
    }

    let json = serde_json::to_value(&by_plan).unwrap();
    assert!(json["tokens"]["groups"][0]["key"].get("reason").is_none());
    assert_eq!(json["tokens"]["groups"][3]["key"]["reason"], "no_plan");
    assert!(json["tokens"]["groups"][3]["key"]["id"].is_null());
}

#[test]
fn analytics_host_filters_widen_within_a_list_and_narrow_across_lists() {
    let ledger = sample_ledger();
    let new_input = |edit: fn(&mut AnalyticsFilters)| {
        let mut filtered = query(AnalyticsGroupBy::Workspace);
        edit(&mut filtered.filters);
        project(&filtered, &ledger)
            .tokens
            .totals
            .map(|totals| totals.new_input)
    };
    assert_eq!(
        new_input(|f| {
            f.workspaces = Some(vec![Some("ws-1".into()), None]);
            f.providers = Some(vec!["prov-a".into()]);
        }),
        Some(13)
    );
    assert_eq!(
        new_input(|f| {
            f.models = Some(vec![ModelKey {
                provider: "prov-a".into(),
                model: None,
            }])
        }),
        Some(20)
    );
    assert_eq!(
        new_input(|f| f.teams = Some(vec![Some("team-1".into())])),
        Some(10)
    );
    assert_eq!(new_input(|f| f.teams = Some(vec![None])), Some(27));
    assert_eq!(new_input(|f| f.plans = Some(vec![None])), Some(3));
    assert_eq!(
        new_input(|f| f.providers = Some(Vec::new())),
        Some(38),
        "an empty list is no filter"
    );
}

#[test]
fn analytics_host_refuses_workspace_team_and_plan_ids_this_machine_never_held() {
    let ledger = sample_ledger();
    let outcome = |edit: fn(&mut AnalyticsFilters), ledger: &LedgerRead| {
        let mut scoped = query(AnalyticsGroupBy::Team);
        edit(&mut scoped.filters);
        project_at(&scoped, ledger, NOW)
            .map(|_| ())
            .map_err(|refusal| refusal.error)
    };
    let foreign: Result<(), AnalyticsRefusalKind> = Err(AnalyticsRefusalKind::ForeignScope);
    assert_eq!(
        outcome(
            |f| f.workspaces = Some(vec![Some("ws-peer".into())]),
            &ledger
        ),
        foreign
    );
    assert_eq!(
        outcome(|f| f.teams = Some(vec![Some("team-peer".into())]), &ledger),
        foreign
    );
    assert_eq!(
        outcome(|f| f.plans = Some(vec![Some("pl-peer".into())]), &ledger),
        foreign
    );
    assert_eq!(
        outcome(
            |f| f.workspaces = Some(vec![Some("ws-gone".into())]),
            &ledger
        ),
        Ok(()),
        "a recorded workspace stays selectable after it is closed"
    );
    assert_eq!(
        outcome(|f| f.teams = Some(vec![Some("team-gone".into())]), &ledger),
        Ok(())
    );
    assert_eq!(
        outcome(
            |f| f.plans = Some(vec![Some("pl-gone".into()), None]),
            &ledger
        ),
        Ok(())
    );
    assert_eq!(
        outcome(
            |f| f.workspaces = Some(vec![Some("ws-1".into())]),
            &LedgerRead::Disabled
        ),
        Ok(())
    );
    assert_eq!(
        outcome(
            |f| f.workspaces = Some(vec![Some("ws-gone".into())]),
            &LedgerRead::Disabled
        ),
        foreign
    );
}

#[test]
fn analytics_host_tab_counts_start_at_their_mark_and_follow_the_workspace_filter_only() {
    let tabs = vec![
        TabCreatedAggregateRow {
            bucket: 1,
            tab_kind: TabCreatedKind::Agent,
            workspace_id: Some("ws-1".into()),
            count: 2,
        },
        TabCreatedAggregateRow {
            bucket: 2,
            tab_kind: TabCreatedKind::Terminal,
            workspace_id: Some("ws-2".into()),
            count: 1,
        },
        TabCreatedAggregateRow {
            bucket: 2,
            tab_kind: TabCreatedKind::Agent,
            workspace_id: None,
            count: 1,
        },
    ];
    let ledger = recorded(
        coverage_from(Some(START), Some(START), Some(START + DAY)),
        Vec::new(),
        tabs,
    );
    let all = project(&query(AnalyticsGroupBy::Workspace), &ledger).tabs_created;
    assert_eq!(all.state, TabsCreatedState::Counted);
    assert_eq!(all.counted_since, Some(START + DAY));
    assert_eq!(all.total, Some(4));
    assert_eq!(
        all.by_kind,
        Some(TabKindCounts {
            agent: 3,
            terminal: 1,
        })
    );
    assert_eq!(
        all.series
            .iter()
            .map(|bucket| (bucket.coverage, bucket.total))
            .collect::<Vec<_>>(),
        vec![
            (Coverage::NotRecorded, None),
            (Coverage::Recorded, Some(2)),
            (Coverage::Recorded, Some(2)),
        ]
    );

    let mut in_beta = query(AnalyticsGroupBy::Workspace);
    in_beta.filters.workspaces = Some(vec![Some("ws-2".into())]);
    let filtered = project(&in_beta, &ledger).tabs_created;
    assert_eq!(filtered.total, Some(1));
    assert_eq!(
        filtered.by_kind,
        Some(TabKindCounts {
            agent: 0,
            terminal: 1,
        })
    );

    let mut by_provider = query(AnalyticsGroupBy::Workspace);
    by_provider.filters.providers = Some(vec!["prov-a".into()]);
    let unattributed = project(&by_provider, &ledger).tabs_created;
    assert_eq!(unattributed.state, TabsCreatedState::NotAttributed);
    assert_eq!(unattributed.total, None);
    assert_eq!(unattributed.by_kind, None);

    let before_counting = recorded(
        coverage_from(Some(START), Some(START), None),
        Vec::new(),
        Vec::new(),
    );
    let uncounted = project(&query(AnalyticsGroupBy::Workspace), &before_counting).tabs_created;
    assert_eq!(uncounted.state, TabsCreatedState::NotCounted);
    assert_eq!(uncounted.total, None);
}

#[test]
fn analytics_host_allowance_stays_account_level_and_money_is_never_recorded() {
    let reading = |weekly: f32| ProviderAllowance {
        captured_at: 1_788_300_000_000,
        session_used_pct: None,
        session_resets_at: None,
        weekly_used_pct: Some(weekly),
        weekly_resets_at: None,
        stale_reason: None,
    };
    let readings = vec![
        ("prov-a".to_string(), reading(40.0)),
        ("prov-b".to_string(), reading(12.5)),
    ];
    let mut scoped = query(AnalyticsGroupBy::Plan);
    scoped.filters.providers = Some(vec!["prov-a".into()]);
    let buckets = buckets();
    let directory = directory();
    let snapshot = project_snapshot(&ProjectionInput {
        query: &scoped,
        buckets: &buckets,
        directory: &directory,
        ledger: &LedgerRead::Disabled,
        allowance: &readings,
        now: NOW,
    })
    .unwrap();
    assert_eq!(snapshot.allowance.len(), 1);
    assert_eq!(snapshot.allowance[0].provider, "prov-a");
    assert_eq!(
        snapshot.allowance[0].attribution,
        AllowanceAttribution::UNAVAILABLE
    );
    assert_eq!(snapshot.money, MoneyBlock::NOT_RECORDED);
    let json = serde_json::to_value(&snapshot).unwrap();
    assert_eq!(json["allowance"][0]["attribution"]["team"], "unavailable");
    assert!(json["money"].get("spend_cents").is_none());
}
