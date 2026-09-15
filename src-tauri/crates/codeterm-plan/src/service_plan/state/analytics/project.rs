use std::collections::{BTreeMap, BTreeSet};

use super::super::mission_control::{ProviderAllowance, KPI_ENABLED_SETTING};
use super::{
    AllowanceAttribution, AllowanceEntry, AnalyticsFilters, AnalyticsGroupBy, AnalyticsQuery,
    AnalyticsRefusal, AnalyticsScope, AnalyticsSnapshot, BucketBound, Coverage, Dimension,
    GroupKey, GroupRow, GroupTotals, KeyReason, LedgerCoverage, LedgerRead, MoneyBlock,
    RecordingBlock, RecordingState, ScopeDirectory, TabBucket, TabCreatedKind, TabKindCounts,
    TabsCreatedBlock, TabsCreatedState, TokenAggregateRow, TokenBlock, TokenBucket, Totals,
    MACHINE_SCOPE,
};

pub struct ProjectionInput<'a> {
    pub query: &'a AnalyticsQuery,
    pub buckets: &'a [BucketBound],
    pub directory: &'a ScopeDirectory,
    pub ledger: &'a LedgerRead,
    pub allowance: &'a [(String, ProviderAllowance)],
    pub now: u64,
}

pub fn project_snapshot(
    input: &ProjectionInput<'_>,
) -> Result<AnalyticsSnapshot, AnalyticsRefusal> {
    refuse_foreign_scope(input)?;
    Ok(AnalyticsSnapshot {
        scope: AnalyticsScope {
            kind: MACHINE_SCOPE,
            includes_mesh_peers: false,
        },
        as_of: input.now,
        range: input.query.range.clone(),
        bucket: input.query.bucket,
        group_by: input.query.group_by,
        recording: recording_block(input.ledger),
        tokens: token_block(input),
        tabs_created: tabs_block(input),
        allowance: allowance_entries(input),
        money: MoneyBlock::NOT_RECORDED,
    })
}

/// Every workspace, team and plan passes the same rule: an id is in scope when this
/// machine holds its record or its ledger recorded it inside the range.
fn refuse_foreign_scope(input: &ProjectionInput<'_>) -> Result<(), AnalyticsRefusal> {
    let directory = input.directory;
    let mut workspaces: BTreeSet<&str> = directory.workspaces.keys().map(String::as_str).collect();
    let mut plans: BTreeSet<&str> = directory.plans.keys().map(String::as_str).collect();
    let mut teams: BTreeSet<&str> = directory.teams.keys().map(String::as_str).collect();
    teams.extend(
        directory
            .plans
            .values()
            .filter_map(|plan| plan.team_id.as_deref()),
    );
    if let LedgerRead::Recorded(aggregates) = input.ledger {
        for row in &aggregates.tokens {
            workspaces.extend(row.workspace_id.as_deref());
            plans.extend(row.plan_id.as_deref());
        }
        for row in &aggregates.tabs {
            workspaces.extend(row.workspace_id.as_deref());
        }
    }
    let filters = &input.query.filters;
    require_known(&filters.workspaces, "workspace", &workspaces)?;
    require_known(&filters.teams, "team", &teams)?;
    require_known(&filters.plans, "plan", &plans)
}

fn require_known(
    values: &Option<Vec<Option<String>>>,
    dimension: &str,
    known: &BTreeSet<&str>,
) -> Result<(), AnalyticsRefusal> {
    for id in values.iter().flatten().flatten() {
        if !known.contains(id.as_str()) {
            return Err(AnalyticsRefusal::foreign(format!(
                "{dimension} {id} is not held by this machine"
            )));
        }
    }
    Ok(())
}

fn recording_block(ledger: &LedgerRead) -> RecordingBlock {
    let empty = RecordingBlock {
        state: RecordingState::Disabled,
        reason: None,
        enable_setting: None,
        first_recorded_at: None,
        last_recorded_at: None,
        workspace_recorded_since: None,
    };
    match ledger {
        LedgerRead::Disabled => RecordingBlock {
            enable_setting: Some(KPI_ENABLED_SETTING),
            ..empty
        },
        LedgerRead::Unavailable { reason } => RecordingBlock {
            state: RecordingState::Unavailable,
            reason: Some(reason.clone()),
            ..empty
        },
        LedgerRead::Recorded(aggregates) => RecordingBlock {
            state: RecordingState::Recording,
            first_recorded_at: aggregates.coverage.first_recorded_at,
            last_recorded_at: aggregates.coverage.last_recorded_at,
            workspace_recorded_since: aggregates.coverage.workspace_recorded_since,
            ..empty
        },
    }
}

/// The ledger upgrade mark proves recording was running even before a first row,
/// which is what lets an idle recorded bucket read as a real zero.
fn recording_since(coverage: &LedgerCoverage) -> Option<u64> {
    match (
        coverage.first_recorded_at,
        coverage.workspace_recorded_since,
    ) {
        (Some(first), Some(mark)) => Some(first.min(mark)),
        (first, mark) => first.or(mark),
    }
}

/// Recording is not known to have covered a bucket before `since` or from `now` on.
fn coverage(bound: &BucketBound, since: Option<u64>, now: u64, has_rows: bool) -> Coverage {
    let covered = match since {
        None => Coverage::NotRecorded,
        Some(_) if bound.start >= now => Coverage::NotRecorded,
        Some(since) if bound.end <= since => Coverage::NotRecorded,
        Some(since) if bound.start < since || bound.end > now => Coverage::Partial,
        Some(_) => Coverage::Recorded,
    };
    if has_rows && covered == Coverage::NotRecorded {
        return Coverage::Partial;
    }
    covered
}

fn within<T>(values: &Option<Vec<T>>, hit: impl Fn(&T) -> bool) -> bool {
    match values {
        Some(values) if !values.is_empty() => values.iter().any(hit),
        _ => true,
    }
}

fn row_matches(filters: &AnalyticsFilters, row: &TokenAggregateRow, team: Option<&str>) -> bool {
    within(&filters.providers, |provider| provider == &row.provider)
        && within(&filters.models, |key| {
            key.provider == row.provider && key.model == row.model
        })
        && within(&filters.workspaces, |workspace| {
            workspace == &row.workspace_id
        })
        && within(&filters.plans, |plan| plan == &row.plan_id)
        && within(&filters.teams, |selected| selected.as_deref() == team)
}

/// A team is read from the plan a row is charged to; nothing else names one.
fn team_of(
    directory: &ScopeDirectory,
    plan_id: Option<&str>,
) -> (Option<String>, Option<KeyReason>) {
    let Some(plan_id) = plan_id else {
        return (None, Some(KeyReason::NoTeam));
    };
    let Some(plan) = directory.plans.get(plan_id) else {
        return (None, Some(KeyReason::PlanDeleted));
    };
    match plan.team_id.as_deref() {
        None => (None, Some(KeyReason::NoTeam)),
        Some(team) if directory.teams.contains_key(team) => (Some(team.to_string()), None),
        Some(team) => (Some(team.to_string()), Some(KeyReason::TeamDeleted)),
    }
}

fn key(dimension: Dimension, id: Option<&str>, reason: Option<KeyReason>) -> GroupKey {
    GroupKey {
        dimension,
        id: id.map(str::to_string),
        provider: None,
        reason,
    }
}

fn provider_key(provider: &str) -> GroupKey {
    if provider.trim().is_empty() {
        return key(
            Dimension::Provider,
            None,
            Some(KeyReason::ProviderNotReported),
        );
    }
    key(Dimension::Provider, Some(provider), None)
}

fn model_key(row: &TokenAggregateRow) -> GroupKey {
    GroupKey {
        dimension: Dimension::Model,
        id: row.model.clone(),
        provider: Some(row.provider.clone()),
        reason: row.model.is_none().then_some(KeyReason::ModelNotReported),
    }
}

fn record_key(
    dimension: Dimension,
    id: Option<&str>,
    held: bool,
    absent: KeyReason,
    deleted: KeyReason,
) -> GroupKey {
    match id {
        None => key(dimension, None, Some(absent)),
        Some(id) => key(dimension, Some(id), (!held).then_some(deleted)),
    }
}

fn group_keys(
    input: &ProjectionInput<'_>,
    row: &TokenAggregateRow,
    team: (Option<String>, Option<KeyReason>),
) -> (GroupKey, Option<GroupKey>) {
    let directory = input.directory;
    match input.query.group_by {
        AnalyticsGroupBy::ProviderModel => (provider_key(&row.provider), Some(model_key(row))),
        AnalyticsGroupBy::Workspace => {
            let id = row.workspace_id.as_deref();
            let held = id.is_some_and(|id| directory.workspaces.contains_key(id));
            let reasons = (KeyReason::WorkspaceNotRecorded, KeyReason::WorkspaceDeleted);
            (
                record_key(Dimension::Workspace, id, held, reasons.0, reasons.1),
                None,
            )
        }
        AnalyticsGroupBy::Plan => {
            let id = row.plan_id.as_deref();
            let held = id.is_some_and(|id| directory.plans.contains_key(id));
            (
                record_key(
                    Dimension::Plan,
                    id,
                    held,
                    KeyReason::NoPlan,
                    KeyReason::PlanDeleted,
                ),
                None,
            )
        }
        AnalyticsGroupBy::Team => (
            GroupKey {
                dimension: Dimension::Team,
                id: team.0,
                provider: None,
                reason: team.1,
            },
            None,
        ),
    }
}

fn label_for(directory: &ScopeDirectory, key: &GroupKey) -> Option<String> {
    let id = key.id.as_deref()?;
    match key.dimension {
        Dimension::Provider => Some(
            directory
                .providers
                .get(id)
                .cloned()
                .unwrap_or_else(|| id.to_string()),
        ),
        Dimension::Model => Some(id.to_string()),
        Dimension::Workspace => directory.workspaces.get(id).cloned(),
        Dimension::Team => directory.teams.get(id).cloned().flatten(),
        Dimension::Plan => directory.plans.get(id).map(|plan| plan.title.clone()),
    }
}

/// Heaviest first; on a tie a resolved id sorts before a null one.
fn rank(a: (&Totals, &GroupKey), b: (&Totals, &GroupKey)) -> std::cmp::Ordering {
    b.0.weight()
        .cmp(&a.0.weight())
        .then_with(|| (a.1.id.is_none(), a.1).cmp(&(b.1.id.is_none(), b.1)))
}

fn ranked_totals(groups: BTreeMap<GroupKey, Totals>) -> Vec<GroupTotals> {
    let mut rows: Vec<GroupTotals> = groups
        .into_iter()
        .map(|(key, totals)| GroupTotals { key, totals })
        .collect();
    rows.sort_by(|a, b| rank((&a.totals, &a.key), (&b.totals, &b.key)));
    rows
}

fn ranked_rows(directory: &ScopeDirectory, groups: BTreeMap<GroupKey, GroupFold>) -> Vec<GroupRow> {
    let mut rows: Vec<GroupRow> = groups
        .into_iter()
        .map(|(key, fold)| GroupRow {
            label: label_for(directory, &key),
            key,
            totals: fold.totals,
            children: fold
                .children
                .into_iter()
                .map(|(key, totals)| GroupRow {
                    label: label_for(directory, &key),
                    key,
                    totals,
                    children: Vec::new(),
                })
                .collect(),
        })
        .collect();
    for row in &mut rows {
        row.children
            .sort_by(|a, b| rank((&a.totals, &a.key), (&b.totals, &b.key)));
    }
    rows.sort_by(|a, b| rank((&a.totals, &a.key), (&b.totals, &b.key)));
    rows
}

#[derive(Default)]
struct GroupFold {
    totals: Totals,
    children: BTreeMap<GroupKey, Totals>,
}

fn token_block(input: &ProjectionInput<'_>) -> TokenBlock {
    let LedgerRead::Recorded(aggregates) = input.ledger else {
        return TokenBlock {
            totals: None,
            series: input
                .buckets
                .iter()
                .map(|bound| TokenBucket {
                    start: bound.start,
                    end: bound.end,
                    label: bound.label.clone(),
                    coverage: Coverage::NotRecorded,
                    totals: None,
                    groups: Vec::new(),
                })
                .collect(),
            groups: Vec::new(),
        };
    };
    let len = input.buckets.len();
    let mut bucket_totals = vec![Totals::default(); len];
    let mut bucket_rows = vec![false; len];
    let mut bucket_groups: Vec<BTreeMap<GroupKey, Totals>> = vec![BTreeMap::new(); len];
    let mut groups: BTreeMap<GroupKey, GroupFold> = BTreeMap::new();
    let mut overall = Totals::default();
    for row in aggregates.tokens.iter().filter(|row| row.bucket < len) {
        let team = team_of(input.directory, row.plan_id.as_deref());
        if !row_matches(&input.query.filters, row, team.0.as_deref()) {
            continue;
        }
        let (top, child) = group_keys(input, row, team);
        overall.add_row(row);
        bucket_totals[row.bucket].add_row(row);
        bucket_rows[row.bucket] = true;
        let leaf = child.clone().unwrap_or_else(|| top.clone());
        bucket_groups[row.bucket]
            .entry(leaf)
            .or_default()
            .add_row(row);
        let fold = groups.entry(top).or_default();
        fold.totals.add_row(row);
        if let Some(child) = child {
            fold.children.entry(child).or_default().add_row(row);
        }
    }
    let since = recording_since(&aggregates.coverage);
    let series: Vec<TokenBucket> = input
        .buckets
        .iter()
        .zip(bucket_groups)
        .enumerate()
        .map(|(index, (bound, groups))| {
            let coverage = coverage(bound, since, input.now, bucket_rows[index]);
            let recorded = coverage != Coverage::NotRecorded;
            TokenBucket {
                start: bound.start,
                end: bound.end,
                label: bound.label.clone(),
                coverage,
                totals: recorded.then_some(bucket_totals[index]),
                groups: ranked_totals(groups),
            }
        })
        .collect();
    let any_recorded = series
        .iter()
        .any(|bucket| bucket.coverage != Coverage::NotRecorded);
    TokenBlock {
        totals: any_recorded.then_some(overall),
        series,
        groups: ranked_rows(input.directory, groups),
    }
}

fn tabs_block(input: &ProjectionInput<'_>) -> TabsCreatedBlock {
    let blank_series = |since: Option<u64>| -> Vec<TabBucket> {
        input
            .buckets
            .iter()
            .map(|bound| TabBucket {
                start: bound.start,
                end: bound.end,
                label: bound.label.clone(),
                coverage: coverage(bound, since, input.now, false),
                total: None,
            })
            .collect()
    };
    let not_counted = || TabsCreatedBlock {
        state: TabsCreatedState::NotCounted,
        counted_since: None,
        total: None,
        by_kind: None,
        series: blank_series(None),
    };
    let LedgerRead::Recorded(aggregates) = input.ledger else {
        return not_counted();
    };
    let Some(since) = aggregates.coverage.tabs_counted_since else {
        return not_counted();
    };
    if input.query.filters.narrows_beyond_workspace() {
        return TabsCreatedBlock {
            state: TabsCreatedState::NotAttributed,
            counted_since: Some(since),
            total: None,
            by_kind: None,
            series: blank_series(Some(since)),
        };
    }
    let len = input.buckets.len();
    let mut per_bucket = vec![0u64; len];
    let mut bucket_rows = vec![false; len];
    let mut kinds = TabKindCounts::default();
    for row in aggregates.tabs.iter().filter(|row| row.bucket < len) {
        if !within(&input.query.filters.workspaces, |workspace| {
            workspace == &row.workspace_id
        }) {
            continue;
        }
        per_bucket[row.bucket] = per_bucket[row.bucket].saturating_add(row.count);
        bucket_rows[row.bucket] = true;
        match row.tab_kind {
            TabCreatedKind::Agent => kinds.agent = kinds.agent.saturating_add(row.count),
            TabCreatedKind::Terminal => kinds.terminal = kinds.terminal.saturating_add(row.count),
        }
    }
    let series: Vec<TabBucket> = input
        .buckets
        .iter()
        .enumerate()
        .map(|(index, bound)| {
            let coverage = coverage(bound, Some(since), input.now, bucket_rows[index]);
            TabBucket {
                start: bound.start,
                end: bound.end,
                label: bound.label.clone(),
                coverage,
                total: (coverage != Coverage::NotRecorded).then_some(per_bucket[index]),
            }
        })
        .collect();
    let any_counted = series
        .iter()
        .any(|bucket| bucket.coverage != Coverage::NotRecorded);
    TabsCreatedBlock {
        state: TabsCreatedState::Counted,
        counted_since: Some(since),
        total: any_counted.then(|| kinds.agent.saturating_add(kinds.terminal)),
        by_kind: any_counted.then_some(kinds),
        series,
    }
}

fn allowance_entries(input: &ProjectionInput<'_>) -> Vec<AllowanceEntry> {
    input
        .allowance
        .iter()
        .filter(|(provider, _)| {
            within(&input.query.filters.providers, |selected| {
                selected == provider
            })
        })
        .map(|(provider, allowance)| AllowanceEntry {
            provider: provider.clone(),
            allowance: allowance.clone(),
            attribution: AllowanceAttribution::UNAVAILABLE,
        })
        .collect()
}
