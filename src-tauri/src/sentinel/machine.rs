use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::provenance::{Classification, Evidence, Scope};

pub const MIN_INTERVAL_MS: u64 = 10_000;
pub const ROUTE_CONFIDENCE: f64 = 0.80;
pub const MAX_BUCKETS: usize = 16;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "target", rename_all = "snake_case")]
pub enum Route {
    Notify,
    Send(String),
    Cron(String),
    Stop,
}

impl Route {
    pub fn parse(value: &str) -> Result<Self, String> {
        match value {
            "notify" => Ok(Self::Notify),
            "stop" => Ok(Self::Stop),
            _ => {
                let (kind, target) = value
                    .split_once(':')
                    .ok_or("route: use notify, send:<tab>, cron:<name>, or stop")?;
                if target.trim().is_empty()
                    || target.len() > 256
                    || target.chars().any(char::is_control)
                {
                    return Err("route: target must be nonempty and bounded".into());
                }
                match kind {
                    "send" => Ok(Self::Send(target.into())),
                    "cron" => Ok(Self::Cron(target.into())),
                    _ => Err("route: use notify, send:<tab>, cron:<name>, or stop".into()),
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Machine {
    pub last_hash: Option<String>,
    pub last_bucket: Option<String>,
    pub stopped: bool,
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
    routes: &BTreeMap<String, Route>,
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
    };
    if machine.stopped {
        return skipped("stopped", evidence);
    }
    let hash = format!("{:x}", Sha256::digest(output));
    evidence.state_hash = Some(hash.clone());
    if machine.last_hash.as_deref() == Some(&hash) {
        return skipped("unchanged", evidence);
    }
    machine.last_hash = Some(hash);
    let classification = classify();
    evidence.classified(classification.source);
    let verdict = match classification.result {
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
    let route = if reason == "transition" {
        machine.last_bucket = Some(verdict.bucket.clone());
        let route = routes
            .get(&verdict.bucket)
            .cloned()
            .unwrap_or(Route::Notify);
        if until == Some(verdict.bucket.as_str()) || route == Route::Stop {
            machine.stopped = true;
        }
        Some(route)
    } else {
        None
    };
    Outcome {
        evidence,
        reason,
        verdict: Some(verdict),
        route,
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
