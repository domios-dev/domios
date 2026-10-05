use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha256};

use super::handlers::Handler;
use super::provenance::{Classification, Evidence, Scope};

pub const MIN_INTERVAL_MS: u64 = 10_000;
pub const ROUTE_CONFIDENCE: f64 = 0.80;
pub const MAX_BUCKETS: usize = 16;

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
    Sentinel(String),
    Stop,
    Command(super::command::Parameters),
    RespondPrompt(SafeKind),
    Escalate(EscalateTarget),
}

impl Route {
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
                    "route: use notify, sentinel:<exact-id>, send:<tab>, cron:<name>, respond_prompt:<kind>, stop, or escalate:creator|agent:<tab>",
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
                    "sentinel"
                        if target.starts_with("sentinel_")
                            && target
                                .chars()
                                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') =>
                    {
                        Ok(Self::Sentinel(target.into()))
                    }
                    _ => Err(
                        "route: use notify, sentinel:<exact-id>, send:<tab>, cron:<name>, or stop"
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
        return Err("--every: minimum sentinel interval is 10s".into());
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
            "sentinel requires a connected Supporter Choice adapter or --match bucket=regex".into(),
        );
    }
    Ok(())
}

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
    let mut evidence = Evidence::new(scope, None);
    let skipped = |reason, evidence| Outcome {
        evidence,
        reason,
        verdict: None,
        route: None,
        handlers: Vec::new(),
    };
    if machine.paused {
        return skipped("paused", evidence);
    }
    if machine.stopped {
        return skipped("stopped", evidence);
    }
    if machine.held.is_some() {
        return skipped("held", evidence);
    }
    let hash = format!("{:x}", Sha256::digest(output));
    evidence.state_hash = Some(hash.clone());
    if machine.last_hash.as_deref() == Some(&hash) {
        return skipped("unchanged", evidence);
    }
    machine.last_hash = Some(hash);
    let classification = classify();
    evidence.classified(classification.source);
    let mut verdict = match classification.result {
        Ok(verdict) => verdict,
        Err(failure) => return skipped(failure.reason(), evidence),
    };
    let reason = if verdict.bucket == "other" || !buckets.contains(&verdict.bucket) {
        "other"
    } else if !verdict.confidence.is_finite()
        || !(ROUTE_CONFIDENCE..=1.0).contains(&verdict.confidence)
    {
        "low_confidence"
    } else if machine.last_bucket.as_deref() == Some(&verdict.bucket) {
        "same_bucket"
    } else {
        "transition"
    };
    let handlers = if reason == "transition" {
        machine.last_bucket = Some(verdict.bucket.clone());
        if until == Some(verdict.bucket.as_str()) {
            machine.stopped = true;
        }
        routes
            .get(&verdict.bucket)
            .cloned()
            .unwrap_or_else(|| vec![Handler::notify(&verdict.bucket)])
    } else if matches!(reason, "other" | "low_confidence") && !on_other.is_empty() {
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
        reason,
        verdict: Some(verdict),
        route,
        handlers,
    }
}

pub fn regex_verdict(patterns: &BTreeMap<String, String>, output: &str) -> Verdict {
    let matches: Vec<_> = patterns
        .iter()
        .filter_map(|(bucket, pattern)| {
            regex::Regex::new(pattern)
                .ok()
                .filter(|regex| regex.is_match(output))
                .map(|_| bucket.clone())
        })
        .collect();
    if matches.len() == 1 {
        Verdict {
            bucket: matches[0].clone(),
            confidence: 1.0,
        }
    } else {
        Verdict {
            bucket: "other".into(),
            confidence: 0.0,
        }
    }
}
