use super::{PortKind, StepDescriptor, MAX_STEP_BYTES};
use crate::converter::ConversionCancellation;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{collections::BTreeMap, time::Instant};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RunScope {
    pub workspace: String,
    pub team: Option<String>,
    pub run: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct InputLineage {
    pub plugin: String,
    pub operation: String,
    pub port: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepContext {
    pub scope: RunScope,
    pub owner: String,
    pub node: String,
    pub attempt: String,
    pub operation: String,
    pub run_view: String,
    pub run_label: String,
    pub node_label: String,
    pub input_lineage: Vec<InputLineage>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Estimate {
    pub amount: u64,
    pub unit: String,
    pub valid_until_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WaitingReason {
    EstimateMissing,
    EstimateStale,
    OverRunCeiling,
    OverStepAllowance,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Waiting {
    pub reason: WaitingReason,
    pub estimate: Option<u64>,
    pub remaining: u64,
    pub unit: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationState {
    Prepared,
    WaitingForOwnerOk,
    Submitting,
    Queued,
    Running,
    Succeeded,
    Failed,
    Canceled,
    Unknown,
}

impl OperationState {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Canceled)
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum PortValue {
    Text { text: String },
    Json { value: Value },
    Media { handle: String, media: PortKind },
    MediaSource { id: String, media: PortKind },
}

pub type PortValues = BTreeMap<String, Vec<PortValue>>;

impl StepDescriptor {
    pub fn validate_values(&self, values: &PortValues, output: bool) -> Result<(), String> {
        if serde_json::to_vec(values)
            .map_err(|_| "invalid values")?
            .len()
            > MAX_STEP_BYTES
        {
            return Err("step payload exceeds 64 KiB".into());
        }
        let ports = if output { &self.outputs } else { &self.inputs };
        if values.keys().any(|key| !ports.contains_key(key)) {
            return Err("undeclared port".into());
        }
        for (name, port) in ports {
            let items = values.get(name).map(Vec::as_slice).unwrap_or_default();
            if items.len() < usize::from(port.min_items)
                || items.len() > usize::from(port.max_items)
            {
                return Err("port cardinality violated".into());
            }
            for value in items {
                match value {
                    PortValue::Text { .. } if port.kind == PortKind::Text => (),
                    PortValue::Json { value } if port.kind == PortKind::Json => {
                        if let Some(schema) = &port.schema {
                            schema
                                .validate_instance(value)
                                .map_err(|_| "JSON port schema violated")?;
                        }
                    }
                    PortValue::Media { handle, media }
                        if *media == port.kind
                            && !matches!(media, PortKind::Text | PortKind::Json)
                            && handle.len() == 64
                            && handle.bytes().all(|b| b.is_ascii_hexdigit()) =>
                    {
                        ()
                    }
                    PortValue::MediaSource { id, media }
                        if output
                            && *media == port.kind
                            && !matches!(media, PortKind::Text | PortKind::Json)
                            && !id.is_empty()
                            && id.len() <= 128
                            && !id.chars().any(char::is_control) =>
                    {
                        ()
                    }
                    _ => return Err("port type violated".into()),
                }
            }
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepRequest {
    pub context: StepContext,
    pub node_type: String,
    pub revision: u32,
    pub params: Value,
    pub inputs: PortValues,
    pub unit: String,
    pub step_allowance: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct JobUpdate {
    pub job: String,
    pub state: OperationState,
    pub outcome: Option<String>,
    pub outputs: PortValues,
}

pub struct StepCall<'a> {
    pub request: &'a StepRequest,
    pub job: Option<&'a str>,
    pub deadline: Instant,
    pub cancellation: ConversionCancellation,
}

pub trait BlueprintStep: Send + Sync {
    fn estimate(&self, call: &StepCall<'_>) -> Result<Option<Estimate>, String>;
    fn submit(&self, call: &StepCall<'_>) -> Result<JobUpdate, String>;
    fn reconcile(&self, call: &StepCall<'_>) -> Result<JobUpdate, String>;
    fn cancel(&self, call: &StepCall<'_>) -> Result<JobUpdate, String>;
}
