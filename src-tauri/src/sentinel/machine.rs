use std::collections::BTreeMap;
use serde::{Deserialize, Serialize};

pub const MIN_INTERVAL_MS: u64 = 10_000;
pub const ROUTE_CONFIDENCE: f64 = 0.80;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "target", rename_all = "snake_case")]
pub enum Route {
    Notify,
    Send(String),
    Cron(String),
    Stop,
}

impl Route {
    pub fn parse(_value: &str) -> Result<Self, String> {
        Err("sentinel routes are not implemented".into())
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
    pub reason: &'static str,
    pub verdict: Option<Verdict>,
    pub route: Option<Route>,
}

pub fn validate(_buckets: &[String], _patterns: &BTreeMap<String, String>, _connected: bool, _interval: u64) -> Result<(), String> {
    Ok(())
}

pub fn evaluate(
    _machine: &mut Machine,
    _output: &[u8],
    _buckets: &[String],
    _routes: &BTreeMap<String, Route>,
    _until: Option<&str>,
    classify: impl FnOnce() -> Result<Verdict, &'static str>,
) -> Outcome {
    Outcome { reason: "pending", verdict: classify().ok(), route: None }
}

pub fn regex_verdict(_patterns: &BTreeMap<String, String>, _output: &str) -> Verdict {
    Verdict { bucket: "other".into(), confidence: 0.0 }
}
