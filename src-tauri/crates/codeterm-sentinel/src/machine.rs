use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::handlers::Handler;
use super::provenance::{Classification, Evidence, Scope};

pub use codeterm_blueprint_types::blueprint_watch::MIN_INTERVAL_MS;
pub const ROUTE_CONFIDENCE: f64 = 0.80;
pub use codeterm_blueprint_types::blueprint_watch::MAX_BUCKETS;

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

pub use codeterm_blueprint_types::blueprint_watch::{EscalateTarget, Route, SafeKind};

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

pub use codeterm_blueprint_types::blueprint_watch::validate_classification as validate;

#[cfg(any(test, feature = "test-support"))]
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

#[cfg(any(test, feature = "test-support"))]
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
        verdict.bucket = codeterm_storage::OTHER_BUCKET.into();
        machine.last_bucket = Some(codeterm_storage::OTHER_BUCKET.into());
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
