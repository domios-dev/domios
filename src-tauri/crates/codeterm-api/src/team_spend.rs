use std::collections::{BTreeMap, HashSet};

use axum::{
    extract::{Path, Query, State as AxumState},
    http::{HeaderMap, StatusCode},
    Json,
};
use chrono::{DateTime, NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::routes::{check_auth, AppState};
use codeterm_service_core::agent::kpi_ledger::DailyTokenUsage;
use codeterm_service_core::plan::PlanStatus;
use codeterm_service_core::service::ServiceContext;
use codeterm_service_core::usage::UsageSnapshotRow;

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
struct SpendPlan {
    id: String,
    title: String,
    team_id: Option<String>,
    status: PlanStatus,
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
    plans: &[SpendPlan],
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
    plans: &[SpendPlan],
    events: &[DailyTokenUsage],
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
        let tokens = event.tokens;
        let Some(day) = event_day(event.timestamp) else {
            continue;
        };
        if !range.contains(day) || !include_model(event.model.as_deref(), model) {
            continue;
        }

        let Some(plan_id) = event.plan_id.as_deref() else {
            attribution.unassigned_plan_rows += event.turns;
            continue;
        };
        if foreign.contains(plan_id) {
            attribution.foreign_plan_rows_excluded += event.turns;
            continue;
        }
        let Some(plan) = team.get(plan_id) else {
            attribution.unknown_plan_rows += event.turns;
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
        value.turns = value.turns.saturating_add(event.turns);
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
        if !reports_usage
            || !codeterm_service_core::config::usage_tracking_enabled_from(
                &svc.configuration(),
                &provider,
            )
        {
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
    let plans_snapshot: Vec<SpendPlan> = svc
        .plan_store
        .list_index()
        .into_iter()
        .map(|entry| SpendPlan {
            id: entry.plan_id,
            title: entry.title,
            team_id: entry.team_id,
            status: entry.status,
        })
        .collect();
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
            .then(|| codeterm_service_core::config::schema::KPI_ENABLED_KEY.to_string()),
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
            let from = range
                .from
                .and_time(chrono::NaiveTime::MIN)
                .and_utc()
                .timestamp()
                .max(0) as u64;
            let to = range
                .to
                .and_time(chrono::NaiveTime::MIN)
                .and_utc()
                .timestamp()
                .saturating_add(86400)
                .max(0) as u64;
            let events = ledger
                .daily_token_usage(from, to)
                .map_err(ProjectionError::Internal)?;
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
    tokio::task::spawn_blocking(move || project_team_spend(&state.svc, &team_id, &query))
        .await
        .map_err(|error| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(serde_json::json!({ "error": error.to_string() })),
            )
        })?
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
mod tests;
