use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use super::handlers::Handler;
use super::provenance::{Classification, Evidence, Scope};

pub const MIN_INTERVAL_MS: u64 = 10_000;
pub const ROUTE_CONFIDENCE: f64 = 0.80;
pub const MAX_BUCKETS: usize = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransitionReason {
    Transition,
    RearmOnChange,
    Other,
    LowConfidence,
    SameBucket,
    Paused,
    Stopped,
    Held,
    Unchanged,
}

impl TransitionReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transition => "transition",
            Self::RearmOnChange => "rearm_on_change",
            Self::Other => "other",
            Self::LowConfidence => "low_confidence",
            Self::SameBucket => "same_bucket",
            Self::Paused => "paused",
            Self::Stopped => "stopped",
            Self::Held => "held",
            Self::Unchanged => "unchanged",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        Some(match value {
            "transition" => Self::Transition,
            "rearm_on_change" => Self::RearmOnChange,
            "other" => Self::Other,
            "low_confidence" => Self::LowConfidence,
            "same_bucket" => Self::SameBucket,
            "paused" => Self::Paused,
            "stopped" => Self::Stopped,
            "held" => Self::Held,
            "unchanged" => Self::Unchanged,
            _ => return None,
        })
    }

    pub fn routes_handlers(self) -> bool {
        matches!(
            self,
            Self::Transition | Self::RearmOnChange | Self::Other | Self::LowConfidence
        )
    }
}

/// Prompt class a respond-prompt handler requests an answer for; the answer path alone
/// chooses among the provider-declared options.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SafeKind {
    DirectoryTrust,
    Onboarding,
    Update,
    Permission,
    Structured,
    Other(String),
}

impl SafeKind {
    pub fn as_str(&self) -> &str {
        match self {
            Self::DirectoryTrust => "directory_trust",
            Self::Onboarding => "onboarding",
            Self::Update => "update",
            Self::Permission => "permission",
            Self::Structured => "structured",
            Self::Other(kind) => kind,
        }
    }
}

impl Serialize for SafeKind {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for SafeKind {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let kind = String::deserialize(deserializer)?;
        Ok(match kind.as_str() {
            "directory_trust" => Self::DirectoryTrust,
            "onboarding" => Self::Onboarding,
            "update" => Self::Update,
            "permission" => Self::Permission,
            "structured" => Self::Structured,
            _ => Self::Other(kind),
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EscalateTarget {
    Creator,
    Agent(String),
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "target",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Route {
    Notify,
    Send(String),
    Cron(String),
    #[serde(rename = "blueprint", alias = "sentinel")]
    Sentinel(String),
    Stop,
    Command(super::command::Parameters),
    RespondPrompt(SafeKind),
    Escalate(EscalateTarget),
}

impl Route {
    pub const DESCRIPTORS:
        &'static [codeterm_blueprint_types::blueprint_catalog::NodeDescriptor] =
        codeterm_blueprint_types::blueprint_catalog::ROUTES;
    pub fn name(&self) -> &'static str {
        Self::DESCRIPTORS[match self {
            Self::Notify => 0,
            Self::Send(_) => 1,
            Self::Cron(_) => 2,
            Self::Command(_) => 3,
            Self::Sentinel(_) => 4,
            Self::Escalate(_) => 5,
            Self::RespondPrompt(_) => 6,
            Self::Stop => 7,
        }]
        .name
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "notify" => Ok(Self::Notify),
            "stop" => Ok(Self::Stop),
            "escalate:creator" => Ok(Self::Escalate(EscalateTarget::Creator)),
            _ if value.starts_with("escalate:agent:") => {
                let target = &value["escalate:agent:".len()..];
                if target.trim().is_empty()
                    || target.len() > 256
                    || target.chars().any(char::is_control)
                    || target.contains('/')
                    || !target
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                {
                    return Err("route: escalation agent must be a bounded tab ID".into());
                }
                Ok(Self::Escalate(EscalateTarget::Agent(target.into())))
            }
            _ => {
                let (kind, target) = value.split_once(':').ok_or(
                    "route: use notify, blueprint:<exact-id>, send:<tab>, cron:<name>, respond_prompt:<kind>, stop, or escalate:creator|agent:<tab>",
                )?;
                if kind == "respond_prompt" {
                    if target.trim().is_empty()
                        || target.len() > 256
                        || target.chars().any(char::is_control)
                    {
                        return Err(
                            "route: respond_prompt kind must be nonempty and bounded".into()
                        );
                    }
                    return Ok(Self::RespondPrompt(
                        serde_json::from_value(serde_json::Value::String(target.into()))
                            .map_err(|_| "route: invalid respond_prompt kind")?,
                    ));
                }
                if target.trim().is_empty()
                    || target.len() > 256
                    || target.chars().any(char::is_control)
                    || target.contains(':')
                    || target.contains('/')
                {
                    return Err("route: target must be nonempty and bounded".into());
                }
                match kind {
                    "send" => Ok(Self::Send(target.into())),
                    "cron" => Ok(Self::Cron(target.into())),
                    "blueprint" | "sentinel"
                        if (target.starts_with("blueprint_")
                            || target.starts_with("sentinel_"))
                            && target
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
                    {
                        Ok(Self::Sentinel(target.into()))
                    }
                    _ => Err(
                        "route: use notify, blueprint:<exact-id>, send:<tab>, cron:<name>, or stop"
                            .into(),
                    ),
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Machine {
    pub last_hash: Option<String>,
    pub last_bucket: Option<String>,
    pub stopped: bool,
    #[serde(default)]
    pub paused: bool,
    /// The `other` transition whose escalation holds the stage.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub held: Option<String>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Verdict {
    pub bucket: String,
    pub confidence: f64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Outcome {
    pub evidence: Evidence,
    pub reason: &'static str,
    pub verdict: Option<Verdict>,
    pub route: Option<Route>,
    pub handlers: Vec<Handler>,
}

pub fn validate(
    buckets: &[String],
    patterns: &BTreeMap<String, String>,
    connected: bool,
    interval: u64,
) -> Result<(), String> {
    if interval < MIN_INTERVAL_MS {
        return Err("--every: minimum blueprint interval is 10s".into());
    }
    if buckets.is_empty() || buckets.len() > MAX_BUCKETS {
        return Err("--buckets: provide between 1 and 16 buckets".into());
    }
    let mut seen = BTreeSet::new();
    for bucket in buckets {
        if bucket == "other"
            || bucket.is_empty()
            || bucket.len() > 64
            || !bucket
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
            || !seen.insert(bucket)
        {
            return Err("--buckets: unique names of up to 64 letters, numbers, underscores or dashes; other is automatic".into());
        }
    }
    for (bucket, pattern) in patterns {
        if !seen.contains(bucket) || pattern.len() > 4096 {
            return Err(
                "--match: bucket must be declared and pattern bounded to 4096 bytes".into(),
            );
        }
        regex::Regex::new(pattern).map_err(|e| format!("--match {bucket}: {e}"))?;
    }
    if !connected && patterns.is_empty() {
        return Err(
            "blueprint requires a connected Supporter Choice adapter or --match bucket=regex"
                .into(),
        );
    }
    Ok(())
}

#[cfg(test)]
pub fn evaluate(
    machine: &mut Machine,
    output: &[u8],
    buckets: &[String],
    routes: &BTreeMap<String, Vec<Handler>>,
    until: Option<&str>,
    scope: Scope,
    classify: impl FnOnce() -> Classification,
) -> Outcome {
    evaluate_with_other(
        machine,
        output,
        buckets,
        routes,
        &[],
        until,
        scope,
        classify,
    )
}

#[cfg(test)]
pub fn evaluate_with_other(
    machine: &mut Machine,
    output: &[u8],
    buckets: &[String],
    routes: &BTreeMap<String, Vec<Handler>>,
    on_other: &[Handler],
    until: Option<&str>,
    scope: Scope,
    classify: impl FnOnce() -> Classification,
) -> Outcome {
    evaluate_with_other_rearm(
        machine,
        output,
        buckets,
        routes,
        &BTreeSet::new(),
        on_other,
        until,
        scope,
        classify,
    )
}

pub fn evaluate_with_other_rearm(
    machine: &mut Machine,
    output: &[u8],
    buckets: &[String],
    routes: &BTreeMap<String, Vec<Handler>>,
    rearm_on_change: &BTreeSet<String>,
    on_other: &[Handler],
    until: Option<&str>,
    scope: Scope,
    classify: impl FnOnce() -> Classification,
) -> Outcome {
    let mut evidence = Evidence::new(scope, None);
    let skipped = |reason, evidence| Outcome {
        evidence,
        reason,
        verdict: None,
        route: None,
        handlers: Vec::new(),
    };
    if machine.paused {
        return skipped(TransitionReason::Paused.as_str(), evidence);
    }
    if machine.stopped {
        return skipped(TransitionReason::Stopped.as_str(), evidence);
    }
    if machine.held.is_some() {
        return skipped(TransitionReason::Held.as_str(), evidence);
    }
    let hash = format!("{:x}", Sha256::digest(output));
    evidence.state_hash = Some(hash.clone());
    if machine.last_hash.as_deref() == Some(&hash) {
        return skipped(TransitionReason::Unchanged.as_str(), evidence);
    }
    machine.last_hash = Some(hash);
    let classification = classify();
    evidence.classified(classification.source);
    evidence.record_decision(&classification);
    let mut verdict = match classification.result {
        Ok(verdict) => verdict,
        Err(failure) => return skipped(failure.reason(), evidence),
    };
    let reason = if verdict.bucket == "other" || !buckets.contains(&verdict.bucket) {
        TransitionReason::Other
    } else if !verdict.confidence.is_finite()
        || !(ROUTE_CONFIDENCE..=1.0).contains(&verdict.confidence)
    {
        TransitionReason::LowConfidence
    } else if machine.last_bucket.as_deref() == Some(&verdict.bucket) {
        if rearm_on_change.contains(&verdict.bucket) {
            TransitionReason::RearmOnChange
        } else {
            TransitionReason::SameBucket
        }
    } else {
        TransitionReason::Transition
    };
    let handlers = if matches!(
        reason,
        TransitionReason::Transition | TransitionReason::RearmOnChange
    ) {
        if reason == TransitionReason::Transition {
            machine.last_bucket = Some(verdict.bucket.clone());
            if until == Some(verdict.bucket.as_str()) {
                machine.stopped = true;
            }
        }
        routes
            .get(&verdict.bucket)
            .cloned()
            .unwrap_or_else(|| vec![Handler::notify(&verdict.bucket)])
    } else if matches!(
        reason,
        TransitionReason::Other | TransitionReason::LowConfidence
    ) && !on_other.is_empty()
    {
        verdict.bucket = crate::storage::OTHER_BUCKET.into();
        machine.last_bucket = Some(crate::storage::OTHER_BUCKET.into());
        let handlers = on_other.to_vec();
        if handlers
            .iter()
            .any(|handler| matches!(&handler.action, Route::Escalate(_)))
        {
            machine.held = Some(evidence.evaluation_id.clone());
        }
        handlers
    } else {
        Vec::new()
    };
    let route = handlers.first().map(|handler| handler.action.clone());
    Outcome {
        evidence,
        reason: reason.as_str(),
        verdict: Some(verdict),
        route,
        handlers,
    }
}

pub fn regex_verdict(patterns: &BTreeMap<String, String>, output: &str) -> Verdict {
    regex_verdict_if_matched(patterns, output).unwrap_or_else(|| Verdict {
        bucket: "other".into(),
        confidence: 0.0,
    })
}

pub fn regex_verdict_if_matched(
    patterns: &BTreeMap<String, String>,
    output: &str,
) -> Option<Verdict> {
    // Windows probes end lines with CRLF, and `(?m)$` only matches before `\n`.
    let normalized = output.replace("\r\n", "\n");
    let output = normalized
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    let matches: Vec<_> = patterns
        .iter()
        .filter_map(|(bucket, pattern)| {
            regex::RegexBuilder::new(pattern)
                .multi_line(true)
                .build()
                .ok()
                .filter(|regex| regex.is_match(&output))
                .map(|_| bucket.clone())
        })
        .collect();
    if matches.is_empty() {
        return None;
    }
    if matches.len() == 1 {
        Some(Verdict {
            bucket: matches[0].clone(),
            confidence: 1.0,
        })
    } else {
        Some(Verdict {
            bucket: "other".into(),
            confidence: 0.0,
        })
    }
}
