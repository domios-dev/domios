use std::collections::{BTreeMap, HashSet};

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::agent::kpi_ledger::{KpiEventKind, KpiEventRecord};
use crate::api::routes::{check_auth, AppState};
use crate::plan::{Plan, PlanStatus};
use crate::service::ServiceContext;
use crate::usage::UsageSnapshotRow;

const MAX_RANGE_DAYS: i64 = 31;
const MAX_ID_CHARS: usize = 256;
const USAGE_HISTORY_LIMIT: i64 = 5_000;
/// Snapshots read before the range so its first day has a baseline to count from.
const WEEKLY_BASELINE_DAYS: i64 = 7;
const RESET_JITTER_MS: i64 = 60_000;

#[derive(Debug, Deserialize)]
pub(crate) struct TeamSpendQuery {
    pub metric: Option<String>,
    pub model: Option<String>,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TeamSpendMetric {
    Tokens,
    WeeklyLimitPoints,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum TeamSpendState {
    Recording,
    NotRecording,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ModelSelector<'a> {
    All,
    One(&'a str),
}

impl ModelSelector<'_> {
    fn response_value(self) -> String {
        match self {
            Self::All => "all".to_string(),
            Self::One(model) => model.to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpendRange {
    from: NaiveDate,
    to: NaiveDate,
}

impl SpendRange {
    fn from_query(query: &TeamSpendQuery) -> Result<Self, String> {
        let from = parse_date(query.from.as_deref(), "from")?;
        let to = parse_date(query.to.as_deref(), "to")?;
        if to < from {
            return Err("to must not be earlier than from".to_string());
        }
        let days = to.signed_duration_since(from).num_days() + 1;
        if days > MAX_RANGE_DAYS {
            return Err(format!("date range must not exceed {MAX_RANGE_DAYS} days"));
        }
        Ok(Self { from, to })
    }

    fn contains(self, day: NaiveDate) -> bool {
        day >= self.from && day <= self.to
    }
}

fn parse_date(value: Option<&str>, name: &str) -> Result<NaiveDate, String> {
    let value = value.ok_or_else(|| format!("{name} is required in YYYY-MM-DD format"))?;
    NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| format!("{name} must use YYYY-MM-DD format"))
}

fn parse_metric(value: Option<&str>) -> Result<TeamSpendMetric, String> {
    match value.unwrap_or("tokens") {
        "tokens" => Ok(TeamSpendMetric::Tokens),
        "weekly_limit_points" => Ok(TeamSpendMetric::WeeklyLimitPoints),
        value => Err(format!("unsupported metric: {value}")),
    }
}

fn parse_model<'a>(value: Option<&'a str>) -> Result<ModelSelector<'a>, String> {
    let Some(value) = value else {
        return Ok(ModelSelector::All);
    };
    if value == "all" {
        return Ok(ModelSelector::All);
    }
    if value.trim().is_empty() {
        return Err("model must be all or a non-empty model id".to_string());
    }
    if value.chars().count() > MAX_ID_CHARS {
        return Err(format!("model must not exceed {MAX_ID_CHARS} characters"));
    }
    Ok(ModelSelector::One(value))
}

fn validate_team_id(team_id: &str) -> Result<(), String> {
    if team_id.trim().is_empty() {
        return Err("team_id must not be empty".to_string());
    }
    if team_id != team_id.trim() {
        return Err("team_id must not have surrounding whitespace".to_string());
    }
    if team_id.chars().count() > MAX_ID_CHARS {
        return Err(format!("team_id must not exceed {MAX_ID_CHARS} characters"));
    }
    Ok(())
}

fn parse_request<'a>(
    team_id: &str,
    query: &'a TeamSpendQuery,
) -> Result<(TeamSpendMetric, ModelSelector<'a>, SpendRange), String> {
    validate_team_id(team_id)?;
    let metric = parse_metric(query.metric.as_deref())?;
    let model = parse_model(query.model.as_deref())?;
    if metric == TeamSpendMetric::WeeklyLimitPoints && matches!(model, ModelSelector::One(_)) {
        return Err("weekly_limit_points has no model attribution; use model=all".to_string());
    }
    Ok((metric, model, SpendRange::from_query(query)?))
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct TeamSpendResponse {
    pub team_id: String,
    pub metric: TeamSpendMetric,
    pub units: &'static str,
    pub scope: &'static str,
    pub model: String,
    pub from: String,
    pub to: String,
    pub state: TeamSpendState,
    pub not_recording_reason: Option<String>,
    /// The setting that turns recording back on, named only while it is off.
    pub enable_setting: Option<String>,
    pub plans: Vec<TeamPlanSummary>,
    pub attribution: AttributionSummary,
    pub daily: Vec<TeamSpendDaily>,
    pub weekly_limit_points: Vec<WeeklyLimitPoint>,
    pub weekly_limit_days: Vec<WeeklyLimitDay>,
}

/// Weekly allowance points one provider account used on one UTC day.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct WeeklyLimitDay {
    pub day: String,
    pub provider: String,
    pub account_id: Option<String>,
    pub points_used: f32,
    pub weekly_limit_points: f32,
    pub scope: WeeklyLimitScope,
    pub team_attribution: &'static str,
    pub model_attribution: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct TeamPlanSummary {
    pub plan_id: String,
    pub title: String,
    pub status: PlanStatus,
}

#[derive(Debug, Clone, Serialize, Default, PartialEq, Eq)]
pub(crate) struct AttributionSummary {
    pub team_plan_count: usize,
    pub unknown_plan_rows: u64,
    pub unassigned_plan_rows: u64,
    pub foreign_plan_rows_excluded: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct TeamSpendDaily {
    pub day: String,
    pub team_id: Option<String>,
    pub plan_id: Option<String>,
    pub plan_title: Option<String>,
    pub plan_status: Option<PlanStatus>,
    pub provider: Option<String>,
    pub model: Option<String>,
    pub input_tokens: u64,
    pub uncached_input_tokens: u64,
    pub output_tokens: u64,
    pub turns: u64,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub(crate) struct WeeklyLimitPoint {
    pub provider: String,
    pub account_id: Option<String>,
    pub weekly_limit_points: f32,
    pub captured_at: i64,
    pub weekly_resets_at: Option<i64>,
    pub scope: WeeklyLimitScope,
    pub team_attribution: &'static str,
    pub model_attribution: &'static str,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub(crate) struct WeeklyLimitScope {
    pub kind: &'static str,
    pub provider: String,
    pub account_id: Option<String>,
}

#[derive(Debug, Clone)]
struct TeamPlanRow {
    summary: TeamPlanSummary,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct DailyKey {
    day: NaiveDate,
    plan_id: Option<String>,
    provider: Option<String>,
    model: Option<String>,
}

#[derive(Debug, Clone)]
struct DailyValue {
    team_id: Option<String>,
    plan_title: Option<String>,
    plan_status: Option<PlanStatus>,
    input_tokens: u64,
    uncached_input_tokens: u64,
    output_tokens: u64,
    turns: u64,
}

#[derive(Debug, Clone)]
struct TokenProjection {
    daily: Vec<TeamSpendDaily>,
    attribution: AttributionSummary,
}

fn plan_summaries(
    plans: &[Plan],
    team_id: &str,
) -> (
    Vec<TeamPlanSummary>,
    BTreeMap<String, TeamPlanRow>,
    HashSet<String>,
) {
    let mut team = BTreeMap::new();
    let mut foreign = HashSet::new();
    for plan in plans {
        match plan.team_id.as_deref() {
            Some(bound_team) if bound_team == team_id => {
                let summary = TeamPlanSummary {
                    plan_id: plan.id.clone(),
                    title: plan.title.clone(),
                    status: plan.status.clone(),
                };
                team.insert(plan.id.clone(), TeamPlanRow { summary });
            }
            Some(_) => {
                foreign.insert(plan.id.clone());
            }
            None => {}
        }
    }
    let summaries = team.values().map(|row| row.summary.clone()).collect();
    (summaries, team, foreign)
}

fn event_day(timestamp: u64) -> Option<NaiveDate> {
    let timestamp = i64::try_from(timestamp).ok()?;
    DateTime::<Utc>::from_timestamp(timestamp, 0).map(|value| value.date_naive())
}

fn snapshot_day(timestamp_ms: i64) -> Option<NaiveDate> {
    DateTime::<Utc>::from_timestamp_millis(timestamp_ms).map(|value| value.date_naive())
}

fn present_identity(value: Option<&str>) -> Option<String> {
    value
        .filter(|value| !value.trim().is_empty())
        .map(str::to_string)
}

fn include_model(model: Option<&str>, selector: ModelSelector<'_>) -> bool {
    match selector {
        ModelSelector::All => true,
        ModelSelector::One(selected) => model == Some(selected),
    }
}

fn project_tokens(
    plans: &[Plan],
    events: &[KpiEventRecord],
    team_id: &str,
    range: SpendRange,
    model: ModelSelector<'_>,
) -> TokenProjection {
    let (summaries, team, foreign) = plan_summaries(plans, team_id);
    let mut values = BTreeMap::<DailyKey, DailyValue>::new();
    let mut attribution = AttributionSummary {
        team_plan_count: summaries.len(),
        ..Default::default()
    };

    for event in events {
        if event.kind != KpiEventKind::TokenUsage {
            continue;
        }
        let Some(tokens) = event.tokens else {
            continue;
        };
        let Some(day) = event_day(event.timestamp) else {
            continue;
        };
        if !range.contains(day) || !include_model(event.model.as_deref(), model) {
            continue;
        }

        let Some(plan_id) = event.plan_id.as_deref() else {
            attribution.unassigned_plan_rows += 1;
            continue;
        };
        if foreign.contains(plan_id) {
            attribution.foreign_plan_rows_excluded += 1;
            continue;
        }
        let Some(plan) = team.get(plan_id) else {
            attribution.unknown_plan_rows += 1;
            continue;
        };
        let plan_id = Some(plan_id.to_string());
        let provider = present_identity(Some(&event.provider));
        let model = present_identity(event.model.as_deref());
        let key = DailyKey {
            day,
            plan_id: plan_id.clone(),
            provider: provider.clone(),
            model: model.clone(),
        };
        let value = values.entry(key).or_insert_with(|| DailyValue {
            team_id: Some(team_id.to_string()),
            plan_title: Some(plan.summary.title.clone()),
            plan_status: Some(plan.summary.status.clone()),
            input_tokens: 0,
            uncached_input_tokens: 0,
            output_tokens: 0,
            turns: 0,
        });
        value.input_tokens = value.input_tokens.saturating_add(tokens.input);
        value.uncached_input_tokens = value
            .uncached_input_tokens
            .saturating_add(tokens.uncached_input);
        value.output_tokens = value.output_tokens.saturating_add(tokens.output);
        value.turns = value.turns.saturating_add(1);
    }

    let daily = values
        .into_iter()
        .map(|(key, value)| TeamSpendDaily {
            day: key.day.format("%Y-%m-%d").to_string(),
            team_id: value.team_id,
            plan_id: key.plan_id,
            plan_title: value.plan_title,
            plan_status: value.plan_status,
            provider: key.provider,
            model: key.model,
            input_tokens: value.input_tokens,
            uncached_input_tokens: value.uncached_input_tokens,
            output_tokens: value.output_tokens,
            turns: value.turns,
        })
        .collect();
    TokenProjection { daily, attribution }
}

fn weekly_limit_points_from_rows(
    rows: Vec<UsageSnapshotRow>,
    range: SpendRange,
) -> Vec<WeeklyLimitPoint> {
    let mut latest_by_account = BTreeMap::<(String, Option<String>), UsageSnapshotRow>::new();
    for row in rows.into_iter().filter(|row| {
        row.parsed_ok && snapshot_day(row.captured_at).is_some_and(|day| range.contains(day))
    }) {
        latest_by_account
            .entry((row.provider.clone(), row.account_id.clone()))
            .and_modify(|latest| {
                if row.captured_at > latest.captured_at {
                    *latest = row.clone();
                }
            })
            .or_insert(row);
    }
    latest_by_account
        .into_values()
        .filter_map(|row| {
            let weekly_limit_points = row.weekly_pct?;
            let account_id = row.account_id.clone();
            let kind = if account_id.is_some() {
                "provider_account"
            } else {
                "machine"
            };
            Some(WeeklyLimitPoint {
                provider: row.provider.clone(),
                account_id: account_id.clone(),
                weekly_limit_points,
                captured_at: row.captured_at,
                weekly_resets_at: row.weekly_resets_at,
                scope: WeeklyLimitScope {
                    kind,
                    provider: row.provider,
                    account_id,
                },
                team_attribution: "unavailable",
                model_attribution: "unavailable",
            })
        })
        .collect()
}

fn scope_of(provider: &str, account_id: Option<&str>) -> WeeklyLimitScope {
    WeeklyLimitScope {
        kind: if account_id.is_some() {
            "provider_account"
        } else {
            "machine"
        },
        provider: provider.to_string(),
        account_id: account_id.map(str::to_string),
    }
}

fn same_weekly_cycle(a: Option<i64>, b: Option<i64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => (a - b).abs() <= RESET_JITTER_MS,
        (None, None) => true,
        _ => false,
    }
}

/// Points used per account per day: the day's last reading less the previous day's,
/// or the whole reading after a reset. A day with no earlier reading is left out.
fn weekly_limit_days_from_rows(
    rows: &[UsageSnapshotRow],
    range: SpendRange,
) -> Vec<WeeklyLimitDay> {
    let mut latest = BTreeMap::<(String, Option<String>, NaiveDate), &UsageSnapshotRow>::new();
    for row in rows
        .iter()
        .filter(|row| row.parsed_ok && row.weekly_pct.is_some())
    {
        let Some(day) = snapshot_day(row.captured_at) else {
            continue;
        };
        latest
            .entry((row.provider.clone(), row.account_id.clone(), day))
            .and_modify(|kept| {
                if row.captured_at > kept.captured_at {
                    *kept = row;
                }
            })
            .or_insert(row);
    }

    let mut days = Vec::new();
    let mut previous: Option<(String, Option<String>, &UsageSnapshotRow)> = None;
    for ((provider, account_id, day), row) in latest {
        let baseline = previous
            .as_ref()
            .filter(|(p, a, _)| *p == provider && *a == account_id)
            .map(|(_, _, row)| *row);
        if let (true, Some(base), Some(reading), Some(base_reading)) = (
            range.contains(day),
            baseline,
            row.weekly_pct,
            baseline.and_then(|base| base.weekly_pct),
        ) {
            let reset = !same_weekly_cycle(row.weekly_resets_at, base.weekly_resets_at)
                || reading < base_reading;
            days.push(WeeklyLimitDay {
                day: day.format("%Y-%m-%d").to_string(),
                provider: provider.clone(),
                account_id: account_id.clone(),
                points_used: if reset {
                    reading
                } else {
                    reading - base_reading
                },
                weekly_limit_points: reading,
                scope: scope_of(&provider, account_id.as_deref()),
                team_attribution: "unavailable",
                model_attribution: "unavailable",
            });
        }
        previous = Some((provider, account_id, row));
    }
    days
}

fn usage_rows(svc: &ServiceContext, range: SpendRange) -> Result<Vec<UsageSnapshotRow>, String> {
    let since_ms = (range.from - chrono::Duration::days(WEEKLY_BASELINE_DAYS))
        .and_hms_opt(0, 0, 0)
        .expect("midnight is valid for a date")
        .and_utc()
        .timestamp_millis();
    let mut rows = Vec::new();
    for provider in svc.agent_registry.provider_ids() {
        let reports_usage = svc
            .agent_registry
            .with_provider(&provider, |registered| registered.usage_parser().is_some())
            .unwrap_or(false);
        if !reports_usage || !crate::config::usage_tracking_enabled(&provider) {
            continue;
        }
        rows.extend(
            svc.usage_store
                .get_history(&provider, since_ms, USAGE_HISTORY_LIMIT, None)?,
        );
    }
    Ok(rows)
}

#[derive(Debug)]
pub(crate) enum ProjectionError {
    BadRequest(String),
    Internal(String),
}

pub(crate) fn project_team_spend(
    svc: &ServiceContext,
    team_id: &str,
    query: &TeamSpendQuery,
) -> Result<TeamSpendResponse, ProjectionError> {
    let (metric, model, range) =
        parse_request(team_id, query).map_err(ProjectionError::BadRequest)?;
    let plans_snapshot = svc.plan_store.list_full();
    let (plans, team_plans, _) = plan_summaries(&plans_snapshot, team_id);
    let base = |state, reason: Option<String>, attribution| TeamSpendResponse {
        team_id: team_id.to_string(),
        metric,
        units: match metric {
            TeamSpendMetric::Tokens => "tokens",
            TeamSpendMetric::WeeklyLimitPoints => "percentage_points",
        },
        scope: match metric {
            TeamSpendMetric::Tokens => "team_plan",
            TeamSpendMetric::WeeklyLimitPoints => "provider_account_or_machine",
        },
        model: model.response_value(),
        from: range.from.format("%Y-%m-%d").to_string(),
        to: range.to.format("%Y-%m-%d").to_string(),
        state,
        enable_setting: (reason.as_deref() == Some("disabled"))
            .then(|| crate::config::schema::KPI_ENABLED_KEY.to_string()),
        not_recording_reason: reason,
        plans: plans.clone(),
        attribution,
        daily: Vec::new(),
        weekly_limit_points: Vec::new(),
        weekly_limit_days: Vec::new(),
    };

    match metric {
        TeamSpendMetric::Tokens => {
            let Some(ledger) = svc.kpi.ledger() else {
                let reason = if svc.kpi.open_failed() {
                    "ledger_unreadable"
                } else {
                    "disabled"
                };
                return Ok(base(
                    TeamSpendState::NotRecording,
                    Some(reason.to_string()),
                    AttributionSummary {
                        team_plan_count: team_plans.len(),
                        ..Default::default()
                    },
                ));
            };
            let events = ledger.list_all().map_err(ProjectionError::Internal)?;
            let projection = project_tokens(&plans_snapshot, &events, team_id, range, model);
            let state = if projection.daily.is_empty() {
                TeamSpendState::NotRecording
            } else {
                TeamSpendState::Recording
            };
            let reason = projection.daily.is_empty().then(|| {
                if team_plans.is_empty() {
                    "no_plans_for_team".to_string()
                } else {
                    "no_rows_for_team".to_string()
                }
            });
            let mut response = base(state, reason, projection.attribution);
            response.daily = projection.daily;
            Ok(response)
        }
        TeamSpendMetric::WeeklyLimitPoints => {
            let rows = usage_rows(svc, range).map_err(ProjectionError::Internal)?;
            let weekly_limit_days = weekly_limit_days_from_rows(&rows, range);
            let weekly_limit_points = weekly_limit_points_from_rows(rows, range);
            let state = if weekly_limit_points.is_empty() {
                TeamSpendState::NotRecording
            } else {
                TeamSpendState::Recording
            };
            let reason = weekly_limit_points
                .is_empty()
                .then(|| "no_weekly_snapshot".to_string());
            let mut response = base(
                state,
                reason,
                AttributionSummary {
                    team_plan_count: team_plans.len(),
                    ..Default::default()
                },
            );
            response.weekly_limit_points = weekly_limit_points;
            response.weekly_limit_days = weekly_limit_days;
            Ok(response)
        }
    }
}

pub(crate) async fn team_spend(
    AxumState(state): AxumState<AppState>,
    headers: HeaderMap,
    Path(team_id): Path<String>,
    Query(query): Query<TeamSpendQuery>,
) -> Result<Json<TeamSpendResponse>, (StatusCode, Json<Value>)> {
    check_auth(&state, &headers)
        .map_err(|(status, message)| (status, Json(serde_json::json!({ "error": message }))))?;
    project_team_spend(&state.svc, &team_id, &query)
        .map(Json)
        .map_err(|error| match error {
            ProjectionError::BadRequest(message) => (
                StatusCode::BAD_REQUEST,
                Json(serde_json::json!({ "error": message })),
            ),
            ProjectionError::Internal(message) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": message })),
            ),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::agent::kpi_ledger::TokenCounts;
    use crate::plan::OrchestratorRef;

    fn range(from: &str, to: &str) -> SpendRange {
        SpendRange::from_query(&TeamSpendQuery {
            metric: Some("tokens".into()),
            model: Some("all".into()),
            from: Some(from.into()),
            to: Some(to.into()),
        })
        .unwrap()
    }

    fn plan(id: &str, title: &str, team_id: Option<&str>, status: PlanStatus) -> Plan {
        let mut plan = Plan::new(
            title.into(),
            "C:/workspace/project".into(),
            OrchestratorRef {
                tab_id: None,
                session_id: None,
                title: None,
            },
        );
        plan.id = id.into();
        plan.team_id = team_id.map(str::to_string);
        plan.status = status;
        plan
    }

    fn event(
        plan_id: Option<&str>,
        provider: &str,
        model: Option<&str>,
        day: &str,
        input: u64,
        uncached_input: u64,
        output: u64,
    ) -> KpiEventRecord {
        let timestamp = NaiveDate::parse_from_str(day, "%Y-%m-%d")
            .unwrap()
            .and_hms_opt(12, 0, 0)
            .unwrap()
            .and_utc()
            .timestamp() as u64;
        KpiEventRecord {
            id: 1,
            schema_version: 3,
            plan_id: plan_id.map(str::to_string),
            task_id: None,
            tab_id: "tab".into(),
            role: None,
            provider: provider.into(),
            model: model.map(str::to_string),
            timestamp,
            kind: KpiEventKind::TokenUsage,
            bytes: input + output,
            tokens: Some(TokenCounts {
                input,
                uncached_input,
                output,
            }),
            dedup_key: None,
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
}
