use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::sentinel_input::{DataRef, MediaKind};

pub const MAX_STEPS: usize = 64;
pub const MAX_BUCKETS: usize = 32;
pub const MAX_BRANCHES_PER_STEP: usize = MAX_BUCKETS + 2;
pub const MAX_COMMAND_BYTES: usize = 16 * 1024;
pub const MAX_RESULT_SUMMARY_CHARS: usize = 200;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintSpec {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    pub name: String,
    #[serde(default)]
    pub owner: BlueprintOwner,
    pub trigger: Trigger,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub result: BlueprintResult,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlueprintOwner {
    #[default]
    Tab,
    Team,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum Trigger {
    Schedule { every: String },
    Event { kind: String, params: Value },
    Input { kind: MediaKind, source: DataRef },
}

impl Trigger {
    pub const DESCRIPTORS: &'static [crate::blueprint_catalog::NodeDescriptor] =
        crate::blueprint_catalog::TRIGGERS;
    pub fn name(&self) -> &'static str {
        Self::DESCRIPTORS[match self {
            Self::Schedule { .. } => 0,
            Self::Event { .. } => 1,
            Self::Input { .. } => 2,
        }]
        .name
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlueprintResult {
    #[serde(default)]
    pub to: ResultTarget,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResultTarget {
    Owner,
    Tab(String),
    Team,
    Notify,
}

impl Default for ResultTarget {
    fn default() -> Self {
        Self::Owner
    }
}

impl Serialize for ResultTarget {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Owner => serializer.serialize_str("owner"),
            Self::Tab(id) => serializer.serialize_str(&format!("tab:{id}")),
            Self::Team => serializer.serialize_str("team"),
            Self::Notify => serializer.serialize_str("notify"),
        }
    }
}

impl<'de> Deserialize<'de> for ResultTarget {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        Ok(match value.as_str() {
            "owner" => Self::Owner,
            "team" => Self::Team,
            "notify" => Self::Notify,
            _ => match value.strip_prefix("tab:") {
                Some(id) if !id.is_empty() => Self::Tab(id.into()),
                _ => {
                    return Err(serde::de::Error::custom(
                        "use owner, team, notify or tab:<id>",
                    ))
                }
            },
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    pub name: String,
    pub input: IoType,
    pub kind: StepKind,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub on: BTreeMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum StepKind {
    Http {
        probe: crate::blueprint_http::HttpProbe,
    },
    Github {
        probe: crate::blueprint_github::GithubProbe,
    },
    Command {
        command: String,
    },
    Ask {
        question: String,
        buckets: Vec<String>,
    },
    Filter {
        pattern: String,
        bucket: Option<String>,
    },
    Convert {
        from: MediaKind,
        to: MediaKind,
    },
    Agent {
        provider: String,
        profile: String,
        task: String,
        deadline_ms: u64,
        max_tokens: u32,
        max_turns: u16,
    },
}

impl StepKind {
    pub const DESCRIPTORS: &'static [crate::blueprint_catalog::NodeDescriptor] =
        crate::blueprint_catalog::STEPS;
    pub fn name(&self) -> &'static str {
        Self::DESCRIPTORS[match self {
            Self::Command { .. } => 0,
            Self::Ask { .. } => 1,
            Self::Filter { .. } => 2,
            Self::Convert { .. } => 3,
            Self::Agent { .. } => 4,
            Self::Github { .. } => 5,
            Self::Http { .. } => 6,
        }]
        .name
    }

    pub fn output_type(&self) -> IoType {
        match self {
            Self::Convert { to, .. } => IoType::from(*to),
            Self::Http { .. }
            | Self::Github { .. }
            | Self::Command { .. }
            | Self::Ask { .. }
            | Self::Filter { .. }
            | Self::Agent { .. } => IoType::Text,
        }
    }

    pub fn outcomes(&self) -> Result<BTreeSet<String>, String> {
        match self {
            Self::Http { .. } => Ok(["up".into(), "down".into(), "failed".into()].into()),
            Self::Github { probe }
                if probe.resource == crate::blueprint_github::GithubResource::Runners =>
            {
                Ok(["online".into(), "offline".into(), "failed".into()].into())
            }
            Self::Github { probe }
                if probe.resource != crate::blueprint_github::GithubResource::PrChecks =>
            {
                Ok(["done".into(), "failed".into(), "failing".into()].into())
            }
            Self::Github { .. }
            | Self::Command { .. }
            | Self::Convert { .. }
            | Self::Agent { .. } => Ok(["done".into(), "failed".into()].into()),
            Self::Ask { buckets, .. } => {
                validate_buckets("buckets", buckets)?;
                Ok(buckets.iter().cloned().chain(["other".into()]).collect())
            }
            Self::Filter { bucket, .. } => {
                let mut outcomes = BTreeSet::from(["done".to_string(), "failed".to_string()]);
                if let Some(bucket) = bucket {
                    validate_bucket("bucket", bucket)?;
                    outcomes.insert(bucket.clone());
                }
                Ok(outcomes)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IoType {
    Text,
    File,
    Audio,
    Video,
    Image,
}

impl From<MediaKind> for IoType {
    fn from(kind: MediaKind) -> Self {
        match kind {
            MediaKind::Text => Self::Text,
            MediaKind::Audio => Self::Audio,
            MediaKind::Video => Self::Video,
            MediaKind::Image => Self::Image,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConverterEdge {
    pub from: MediaKind,
    pub to: MediaKind,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ValidationError {
    pub field: String,
    pub reason: String,
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.reason)
    }
}

pub fn validate(
    spec: &BlueprintSpec,
    converters: &[ConverterEdge],
) -> Result<(), Vec<ValidationError>> {
    let mut errors = Vec::new();
    if let Err(reason) = crate::automation_name::validate_note(spec.note.as_deref()) {
        errors.push(ValidationError {
            field: "note".into(),
            reason,
        });
    }
    if let Err(reason) = crate::automation_name::validate_sentinel_name(&spec.name) {
        error(&mut errors, "name", &reason);
    }
    match &spec.trigger {
        Trigger::Schedule { every } if !valid_duration(every) => {
            error(&mut errors, "trigger.every", "must be a positive duration")
        }
        Trigger::Event { kind, .. } if !valid_name(kind) => error(
            &mut errors,
            "trigger.kind",
            "must be a bounded capability kind",
        ),
        Trigger::Input { kind, source } => {
            let input = crate::sentinel_input::InputSpec {
                kind: *kind,
                data: source.clone(),
                convert_to: None,
                convert_timeout_ms: None,
            };
            if let Err(reason) = crate::sentinel_input::validate(&input) {
                error(&mut errors, "trigger", reason.as_str());
            }
        }
        _ => {}
    }
    if let ResultTarget::Tab(id) = &spec.result.to {
        if !valid_name(id) {
            error(
                &mut errors,
                "result.to",
                "tab id must be nonempty and bounded",
            );
        }
    }
    if spec.steps.is_empty() || spec.steps.len() > MAX_STEPS {
        error(
            &mut errors,
            "steps",
            &format!("must contain 1..={MAX_STEPS} steps"),
        );
        if spec.steps.len() > MAX_STEPS {
            return Err(errors);
        }
    }
    let names: BTreeMap<_, _> = spec
        .steps
        .iter()
        .enumerate()
        .map(|(index, step)| (step.name.as_str(), index))
        .collect();
    let mut seen = BTreeSet::new();
    let mut previous = trigger_output(&spec.trigger);
    for (index, step) in spec.steps.iter().enumerate() {
        let field = format!("steps[{index}]");
        if !valid_name(&step.name) {
            error(
                &mut errors,
                &format!("{field}.name"),
                "must be nonempty and bounded",
            );
        }
        if !seen.insert(step.name.as_str()) {
            error(&mut errors, &format!("{field}.name"), "must be unique");
        }
        if step.input != previous {
            error(
                &mut errors,
                &format!("{field}.input"),
                &format!("expected {previous:?} from the previous result"),
            );
        }
        match &step.kind {
            StepKind::Http { probe } => {
                if step.input != IoType::Text {
                    error(
                        &mut errors,
                        &format!("{field}.input"),
                        "HTTP requires text input",
                    );
                }
                if let Err(reason) = probe.validate() {
                    error(&mut errors, &format!("{field}.http"), &reason);
                }
            }
            StepKind::Github { probe } => {
                if step.input != IoType::Text {
                    error(
                        &mut errors,
                        &format!("{field}.input"),
                        "GitHub requires text input",
                    );
                }
                if let Err(reason) = probe.command() {
                    error(&mut errors, &format!("{field}.github"), &reason);
                }
            }
            StepKind::Agent {
                provider,
                profile,
                task,
                deadline_ms,
                max_tokens,
                max_turns,
            } => {
                if provider.is_empty()
                    || provider.len() > 128
                    || profile.is_empty()
                    || profile.len() > 128
                    || task.is_empty()
                    || task.len() > 32768
                    || task.contains('\0')
                    || *deadline_ms == 0
                    || *deadline_ms > 3_600_000
                    || *max_tokens == 0
                    || *max_tokens > 1_000_000
                    || *max_turns == 0
                    || *max_turns > 100
                    || step.input != IoType::Text
                {
                    error(&mut errors, &format!("{field}.agent"), "agent requires text, a provider and profile, and bounded task, deadline, token and turn budgets");
                }
            }
            StepKind::Command { command }
                if command.trim().is_empty()
                    || command.len() > MAX_COMMAND_BYTES
                    || command.contains('\0') =>
            {
                error(
                    &mut errors,
                    &format!("{field}.command"),
                    "must be nonempty and bounded",
                )
            }
            StepKind::Ask { question, .. }
                if question.trim().is_empty() || question.len() > 2048 =>
            {
                error(
                    &mut errors,
                    &format!("{field}.question"),
                    "must be nonempty and bounded",
                )
            }
            StepKind::Filter { pattern, .. } if pattern.is_empty() || pattern.len() > 4096 => {
                error(
                    &mut errors,
                    &format!("{field}.pattern"),
                    "must be nonempty and bounded",
                )
            }
            StepKind::Convert { from, to } => {
                if step.input != IoType::from(*from) {
                    error(
                        &mut errors,
                        &format!("{field}.from"),
                        "must match the step input type",
                    );
                }
                if !has_converter_path(*from, *to, converters) {
                    error(
                        &mut errors,
                        &format!("{field}.convert"),
                        "no converter chain is available",
                    );
                }
            }
            _ => {}
        }
        if matches!(&step.kind, StepKind::Ask { .. } | StepKind::Filter { .. })
            && step.input != IoType::Text
        {
            error(
                &mut errors,
                &format!("{field}.input"),
                "ask and filter require text",
            );
        }
        match step.kind.outcomes() {
            Ok(outcomes) => {
                if step.on.len() > MAX_BRANCHES_PER_STEP {
                    error(
                        &mut errors,
                        &format!("{field}.on"),
                        &format!("exceeds {MAX_BRANCHES_PER_STEP} outcomes"),
                    );
                }
                for (outcome, target) in step.on.iter().take(MAX_BRANCHES_PER_STEP) {
                    if !outcomes.contains(outcome) {
                        error(
                            &mut errors,
                            &format!("{field}.on.{outcome}"),
                            "outcome is not declared by this step",
                        );
                    }
                    if target != "next" && target != "stop" {
                        match names.get(target.as_str()) {
                            Some(target_index) if *target_index > index => {}
                            Some(_) => error(
                                &mut errors,
                                &format!("{field}.on.{outcome}"),
                                "branch targets must be forward-only",
                            ),
                            None => error(
                                &mut errors,
                                &format!("{field}.on.{outcome}"),
                                "branch target step does not exist",
                            ),
                        }
                    }
                }
            }
            Err(reason) => {
                let field_name = if matches!(&step.kind, StepKind::Ask { .. }) {
                    "buckets"
                } else {
                    "bucket"
                };
                error(&mut errors, &format!("{field}.{field_name}"), &reason)
            }
        }
        previous = step.kind.output_type();
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

pub fn trigger_unavailable(trigger: &Trigger) -> bool {
    !matches!(
        trigger,
        Trigger::Schedule { .. } | Trigger::Event { .. } | Trigger::Input { .. }
    )
}

pub fn ensure_trigger_available(trigger: &Trigger) -> Result<(), ValidationError> {
    if trigger_unavailable(trigger) {
        Err(ValidationError {
            field: "trigger".into(),
            reason: "trigger_unavailable".into(),
        })
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionOwnerState {
    Active,
    Stopped,
    Expired,
    OwnerClosed,
    TeamClosed,
    Amended,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionProviderState {
    Available,
    Missing,
    Ungranted,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SubscriptionAction {
    Subscribe,
    Unsubscribe,
    Unavailable,
    Keep,
}

pub fn next_subscription_action(
    subscribed: bool,
    owner: SubscriptionOwnerState,
    provider: SubscriptionProviderState,
) -> SubscriptionAction {
    if owner != SubscriptionOwnerState::Active {
        return if subscribed {
            SubscriptionAction::Unsubscribe
        } else {
            SubscriptionAction::Keep
        };
    }
    match provider {
        SubscriptionProviderState::Available if subscribed => SubscriptionAction::Keep,
        SubscriptionProviderState::Available => SubscriptionAction::Subscribe,
        SubscriptionProviderState::Missing | SubscriptionProviderState::Ungranted => {
            if subscribed {
                SubscriptionAction::Unsubscribe
            } else {
                SubscriptionAction::Unavailable
            }
        }
    }
}

pub fn converter_path(
    from: MediaKind,
    to: MediaKind,
    converters: &[ConverterEdge],
) -> Option<Vec<usize>> {
    if from == to {
        return Some(Vec::new());
    }
    let mut frontier: Vec<(MediaKind, Vec<usize>)> = vec![(from, Vec::new())];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for (kind, path) in frontier {
            if path.len() >= 3 {
                continue;
            }
            for (index, edge) in converters
                .iter()
                .enumerate()
                .filter(|(_, edge)| edge.from == kind)
            {
                if path.iter().any(|used| converters[*used].from == edge.to) {
                    continue;
                }
                let mut candidate = path.clone();
                candidate.push(index);
                if edge.to == to {
                    return Some(candidate);
                }
                next.push((edge.to, candidate));
            }
        }
        frontier = next;
    }
    None
}

fn has_converter_path(from: MediaKind, to: MediaKind, converters: &[ConverterEdge]) -> bool {
    converter_path(from, to, converters).is_some()
}

fn trigger_output(trigger: &Trigger) -> IoType {
    match trigger {
        Trigger::Schedule { .. } | Trigger::Event { .. } => IoType::Text,
        Trigger::Input { kind, .. } => IoType::from(*kind),
    }
}

fn validate_buckets(field: &str, buckets: &[String]) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    if buckets.is_empty() || buckets.len() > MAX_BUCKETS {
        return Err(format!("{field} must contain 1..={MAX_BUCKETS} names"));
    }
    for bucket in buckets {
        validate_bucket(field, bucket)?;
        if bucket == "other" || !seen.insert(bucket) {
            return Err(format!("{field} contains a reserved or duplicate name"));
        }
    }
    Ok(())
}

fn validate_bucket(field: &str, bucket: &str) -> Result<(), String> {
    if valid_name(bucket) && bucket != "other" {
        Ok(())
    } else {
        Err(format!("{field} must be a bounded name other than other"))
    }
}

fn valid_name(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 128 && !has_control(value)
}

fn has_control(value: &str) -> bool {
    value.chars().any(char::is_control)
}

fn valid_duration(value: &str) -> bool {
    value.ends_with(['s', 'm', 'h', 'd'])
        && crate::blueprint_watch::duration_ms(value)
            .is_ok_and(|duration| duration >= crate::blueprint_watch::MIN_INTERVAL_MS)
}

fn error(errors: &mut Vec<ValidationError>, field: &str, reason: &str) {
    errors.push(ValidationError {
        field: field.into(),
        reason: reason.into(),
    });
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunnerState {
    pub current_step: Option<usize>,
    pub status: RunStatus,
    step_names: Vec<String>,
    step_outcomes: Vec<BTreeSet<String>>,
    branches: Vec<BTreeMap<String, String>>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunStatus {
    Running,
    Done,
    Failed,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    Done,
    Failed,
    Bucket(String),
}

impl StepOutcome {
    fn key(&self) -> &str {
        match self {
            Self::Done => "done",
            Self::Failed => "failed",
            Self::Bucket(bucket) => bucket,
        }
    }
}

impl RunnerState {
    pub fn start(spec: &BlueprintSpec) -> Result<Self, String> {
        if spec.steps.is_empty() || spec.steps.len() > MAX_STEPS {
            return Err("steps must contain a bounded nonempty chain".into());
        }
        Ok(Self {
            current_step: Some(0),
            status: RunStatus::Running,
            step_names: spec.steps.iter().map(|step| step.name.clone()).collect(),
            step_outcomes: spec
                .steps
                .iter()
                .map(|step| step.kind.outcomes())
                .collect::<Result<_, _>>()?,
            branches: spec.steps.iter().map(|step| step.on.clone()).collect(),
        })
    }
}

pub fn advance(state: &RunnerState, outcome: StepOutcome) -> Result<RunnerState, String> {
    if state.status != RunStatus::Running {
        return Err("runner is not active".into());
    }
    let index = state.current_step.ok_or("runner has no current step")?;
    let accepted = state
        .step_outcomes
        .get(index)
        .ok_or("current step is outside the chain")?;
    let key = outcome.key();
    if !accepted.contains(key) {
        return Err("outcome is not declared by the current step".into());
    }
    let branch = state.branches[index]
        .get(key)
        .map(String::as_str)
        .unwrap_or_else(|| if key == "failed" { "stop" } else { "next" });
    let mut next = state.clone();
    match branch {
        "stop" if key == "failed" => {
            next.current_step = None;
            next.status = RunStatus::Failed;
        }
        "stop" => {
            next.current_step = None;
            next.status = RunStatus::Done;
        }
        "next" if index + 1 < state.step_names.len() => next.current_step = Some(index + 1),
        "next" => {
            next.current_step = None;
            next.status = RunStatus::Done;
        }
        target => {
            let target = state
                .step_names
                .iter()
                .position(|name| name == target)
                .ok_or("branch target does not exist")?;
            if target <= index {
                return Err("branch target must be forward-only".into());
            }
            next.current_step = Some(target);
        }
    }
    Ok(next)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JournalStepState {
    Waiting,
    Running,
    Done,
    Failed,
    Skipped,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StepJournalEntry {
    pub step_index: usize,
    pub step_name: String,
    pub step_kind: String,
    pub state: JournalStepState,
    pub outcome: Option<String>,
    pub started_at_ms: Option<u64>,
    pub ended_at_ms: Option<u64>,
    pub result_summary: Option<String>,
    pub reason: Option<String>,
}

impl StepJournalEntry {
    pub fn bound_summary(value: &str) -> String {
        value
            .lines()
            .next()
            .unwrap_or_default()
            .chars()
            .filter(|character| !character.is_control())
            .take(MAX_RESULT_SUMMARY_CHARS)
            .collect()
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.step_name.trim().is_empty()
            || self.step_name.len() > 128
            || self.step_name.chars().any(char::is_control)
        {
            return Err("step_name must be nonempty, bounded and printable".into());
        }
        if !matches!(
            self.step_kind.as_str(),
            "command" | "convert" | "ask" | "filter" | "agent"
        ) {
            return Err("step_kind is not declared".into());
        }
        if self
            .result_summary
            .as_ref()
            .is_some_and(|summary| summary.chars().count() > MAX_RESULT_SUMMARY_CHARS)
        {
            return Err("result_summary exceeds 200 characters".into());
        }
        if self
            .result_summary
            .as_ref()
            .is_some_and(|summary| summary.chars().any(char::is_control))
        {
            return Err("result_summary contains control characters".into());
        }
        if self
            .started_at_ms
            .zip(self.ended_at_ms)
            .is_some_and(|(start, end)| end < start)
        {
            return Err("ended_at_ms precedes started_at_ms".into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests;
