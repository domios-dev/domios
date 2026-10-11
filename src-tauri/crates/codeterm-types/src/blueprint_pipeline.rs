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
        }]
        .name
    }

    pub fn output_type(&self) -> IoType {
        match self {
            Self::Convert { to, .. } => IoType::from(*to),
            Self::Command { .. } | Self::Ask { .. } | Self::Filter { .. } | Self::Agent { .. } => {
                IoType::Text
            }
        }
    }

    pub fn outcomes(&self) -> Result<BTreeSet<String>, String> {
        match self {
            Self::Command { .. } | Self::Convert { .. } | Self::Agent { .. } => {
                Ok(["done".into(), "failed".into()].into())
            }
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
    if spec.name.trim().is_empty() || spec.name.len() > 128 || has_control(&spec.name) {
        error(
            &mut errors,
            "name",
            "must be nonempty, bounded and printable",
        );
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
                if command.trim().is_empty() || command.len() > MAX_COMMAND_BYTES =>
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
    if value.is_empty() || value.len() > 32 {
        return false;
    }
    let mut number = 0u64;
    let mut has_digits = false;
    let mut total = 0u64;
    for byte in value.bytes() {
        if byte.is_ascii_digit() {
            has_digits = true;
            let Some(next) = number
                .checked_mul(10)
                .and_then(|value| value.checked_add((byte - b'0') as u64))
            else {
                return false;
            };
            number = next;
            continue;
        }
        let factor = match byte {
            b's' => 1,
            b'm' => 60,
            b'h' => 3_600,
            b'd' => 86_400,
            _ => return false,
        };
        if !has_digits {
            return false;
        }
        let Some(next) = number
            .checked_mul(factor)
            .and_then(|value| total.checked_add(value))
        else {
            return false;
        };
        total = next;
        number = 0;
        has_digits = false;
    }
    !has_digits && total >= 10
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
mod tests {
    use super::*;

    #[test]
    fn blueprint_agent_step_is_bounded_and_routes_done_or_failed() {
        let agent = StepKind::Agent {
            provider: "fixture".into(),
            profile: "reader".into(),
            task: "opaque".into(),
            deadline_ms: 1000,
            max_tokens: 1000,
            max_turns: 2,
        };
        assert_eq!(agent.name(), "agent");
        assert_eq!(agent.output_type(), IoType::Text);
        assert_eq!(
            agent.outcomes().unwrap(),
            BTreeSet::from(["done".into(), "failed".into()])
        );
        assert!(serde_json::from_value::<StepKind>(serde_json::json!({"kind":"agent","provider":"fixture","profile":"reader","task":"opaque","deadline_ms":1000,"max_tokens":1000,"max_turns":2,"unknown":true})).is_err());
        let mut step = command("bounded", IoType::Text);
        step.kind = agent;
        assert!(validate(&spec(vec![step.clone()]), &[]).is_ok());
        if let StepKind::Agent { max_turns, .. } = &mut step.kind {
            *max_turns = 0;
        }
        assert!(validate(&spec(vec![step]), &[])
            .unwrap_err()
            .iter()
            .any(|e| e.field == "steps[0].agent"));
    }
    use crate::sentinel_input::DataRef;

    fn command(name: &str, input: IoType) -> Step {
        Step {
            name: name.into(),
            input,
            kind: StepKind::Command {
                command: "printf ready".into(),
            },
            on: BTreeMap::new(),
        }
    }

    fn spec(steps: Vec<Step>) -> BlueprintSpec {
        BlueprintSpec {
            name: "Echo chain".into(),
            owner: BlueprintOwner::Tab,
            trigger: Trigger::Schedule {
                every: "20s".into(),
            },
            steps,
            result: BlueprintResult {
                to: ResultTarget::Owner,
            },
        }
    }

    #[test]
    fn blueprint_pipeline_spec_validation_checks_name_trigger_and_step_bounds() {
        let mut value = spec(vec![command("run", IoType::Text)]);
        assert_eq!(validate(&value, &[]), Ok(()));
        value.name.clear();
        value.trigger = Trigger::Schedule { every: "0s".into() };
        let errors = validate(&value, &[]).unwrap_err();
        assert!(errors.iter().any(|error| error.field == "name"));
        assert!(errors.iter().any(|error| error.field == "trigger.every"));
    }

    #[test]
    fn blueprint_pipeline_spec_round_trips_one_authored_chain() {
        let source = serde_json::json!({
            "name": "Echo chain",
            "owner": "tab",
            "trigger": {"type": "schedule", "every": "20s"},
            "steps": [{
                "name": "echo",
                "input": "text",
                "kind": {"kind": "command", "command": "printf ready"}
            }],
            "result": {"to": "owner"}
        });
        let parsed: BlueprintSpec = serde_json::from_value(source.clone()).unwrap();
        assert_eq!(validate(&parsed, &[]), Ok(()));
        assert_eq!(serde_json::to_value(parsed).unwrap(), source);
    }

    #[test]
    fn blueprint_pipeline_forward_only_branches_reject_missing_and_backward_targets() {
        let mut first = command("first", IoType::Text);
        first.on.insert("done".into(), "first".into());
        assert!(validate(
            &spec(vec![first.clone(), command("second", IoType::Text)]),
            &[]
        )
        .unwrap_err()
        .iter()
        .any(|error| error.reason.contains("forward-only")));
        first.on.insert("done".into(), "missing".into());
        assert!(
            validate(&spec(vec![first, command("second", IoType::Text)]), &[])
                .unwrap_err()
                .iter()
                .any(|error| error.reason.contains("does not exist"))
        );
        let mut first = command("first", IoType::Text);
        first.on.insert("done".into(), "third".into());
        assert_eq!(
            validate(
                &spec(vec![
                    first,
                    command("second", IoType::Text),
                    command("third", IoType::Text)
                ]),
                &[]
            ),
            Ok(())
        );
    }

    #[test]
    fn blueprint_pipeline_step_io_types_match_previous_output() {
        let mut second = command("second", IoType::File);
        second.kind = StepKind::Filter {
            pattern: "ready".into(),
            bucket: None,
        };
        let errors =
            validate(&spec(vec![command("first", IoType::Text), second]), &[]).unwrap_err();
        assert!(errors.iter().any(|error| error.field == "steps[1].input"));
    }

    #[test]
    fn blueprint_pipeline_converter_chain_is_capability_ordered_and_bounded() {
        let converters = [
            ConverterEdge {
                from: MediaKind::Audio,
                to: MediaKind::Video,
            },
            ConverterEdge {
                from: MediaKind::Video,
                to: MediaKind::Text,
            },
        ];
        assert_eq!(
            converter_path(MediaKind::Audio, MediaKind::Text, &converters),
            Some(vec![0, 1])
        );
        for kind in MediaKind::ALL {
            assert_eq!(converter_path(kind, kind, &converters), Some(vec![]));
            assert_eq!(converter_path(kind, kind, &[]), Some(vec![]));
        }
        let spec = BlueprintSpec {
            name: "Voice".into(),
            owner: BlueprintOwner::Tab,
            trigger: Trigger::Input {
                kind: MediaKind::Audio,
                source: DataRef::Url("https://example.com/voice.wav".into()),
            },
            steps: vec![
                Step {
                    name: "convert".into(),
                    input: IoType::Audio,
                    kind: StepKind::Convert {
                        from: MediaKind::Audio,
                        to: MediaKind::Text,
                    },
                    on: BTreeMap::new(),
                },
                command("inspect", IoType::Text),
            ],
            result: BlueprintResult {
                to: ResultTarget::Owner,
            },
        };
        assert_eq!(validate(&spec, &converters), Ok(()));
    }

    #[test]
    fn blueprint_pipeline_runner_advance_table() {
        for (outcome, expected, status) in [
            (StepOutcome::Done, Some(1), RunStatus::Running),
            (StepOutcome::Failed, None, RunStatus::Failed),
        ] {
            let state = RunnerState::start(&spec(vec![
                command("one", IoType::Text),
                command("two", IoType::Text),
            ]))
            .unwrap();
            let advanced = advance(&state, outcome).unwrap();
            assert_eq!(advanced.current_step, expected);
            assert_eq!(advanced.status, status);
        }
        let mut ask = Step {
            name: "choose".into(),
            input: IoType::Text,
            kind: StepKind::Ask {
                question: "State?".into(),
                buckets: vec!["ready".into()],
            },
            on: BTreeMap::new(),
        };
        ask.on.insert("ready".into(), "stop".into());
        let state = RunnerState::start(&spec(vec![ask])).unwrap();
        let advanced = advance(&state, StepOutcome::Bucket("ready".into())).unwrap();
        assert_eq!(advanced.status, RunStatus::Done);

        let mut first = command("one", IoType::Text);
        first.on.insert("done".into(), "three".into());
        let state = RunnerState::start(&spec(vec![
            first,
            command("two", IoType::Text),
            command("three", IoType::Text),
        ]))
        .unwrap();
        assert_eq!(
            advance(&state, StepOutcome::Done).unwrap().current_step,
            Some(2)
        );
    }

    #[test]
    fn blueprint_pipeline_rejects_undeclared_buckets_and_branch_outcomes() {
        let mut ask = Step {
            name: "choose".into(),
            input: IoType::Text,
            kind: StepKind::Ask {
                question: "State?".into(),
                buckets: vec!["ready".into()],
            },
            on: BTreeMap::new(),
        };
        ask.on.insert("failed".into(), "stop".into());
        let errors = validate(&spec(vec![ask]), &[]).unwrap_err();
        assert!(errors
            .iter()
            .any(|error| error.field == "steps[0].on.failed"));
        let invalid = Step {
            name: "choose".into(),
            input: IoType::Text,
            kind: StepKind::Ask {
                question: "State?".into(),
                buckets: vec!["other".into()],
            },
            on: BTreeMap::new(),
        };
        assert!(validate(&spec(vec![invalid]), &[])
            .unwrap_err()
            .iter()
            .any(|error| error.field == "steps[0].buckets"));
    }

    #[test]
    fn blueprint_pipeline_event_triggers_are_available_and_input_waits_for_runtime() {
        assert!(ensure_trigger_available(&Trigger::Schedule {
            every: "20s".into()
        })
        .is_ok());
        assert!(ensure_trigger_available(&Trigger::Event {
            kind: "fixture.event".into(),
            params: Value::Null,
        })
        .is_ok());
        assert!(ensure_trigger_available(&Trigger::Input {
            kind: MediaKind::Text,
            source: DataRef::Text("hello".into()),
        })
        .is_ok());
    }

    #[test]
    fn blueprint_event_subscription_action_table_covers_owner_and_provider_states() {
        use SubscriptionAction::*;
        use SubscriptionOwnerState::*;
        use SubscriptionProviderState::*;
        for (subscribed, owner, provider, expected) in [
            (false, Active, Available, Subscribe),
            (true, Active, Available, Keep),
            (false, Active, Missing, Unavailable),
            (false, Active, Ungranted, Unavailable),
            (true, Active, Missing, Unsubscribe),
            (true, Active, Ungranted, Unsubscribe),
            (true, Stopped, Available, Unsubscribe),
            (true, Expired, Available, Unsubscribe),
            (true, OwnerClosed, Available, Unsubscribe),
            (true, TeamClosed, Available, Unsubscribe),
            (true, Amended, Available, Unsubscribe),
            (false, Stopped, Available, Keep),
        ] {
            assert_eq!(
                next_subscription_action(subscribed, owner, provider),
                expected
            );
        }
    }

    #[test]
    fn blueprint_pipeline_journal_summary_is_bounded_and_control_free() {
        let entry = StepJournalEntry {
            step_index: 0,
            step_name: "run".into(),
            step_kind: "command".into(),
            state: JournalStepState::Done,
            outcome: Some("done".into()),
            started_at_ms: Some(1),
            ended_at_ms: Some(2),
            result_summary: Some(StepJournalEntry::bound_summary(&format!(
                "{}\n{}",
                "x".repeat(199),
                "tail"
            ))),
            reason: None,
        };
        entry.validate().unwrap();
        let expected = "x".repeat(199);
        assert_eq!(entry.result_summary.as_deref(), Some(expected.as_str()));
        assert_eq!(StepJournalEntry::bound_summary("a\u{1b}b"), "ab");
    }
}
