use super::StepSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

pub const MAX_STEP_DESCRIPTORS: usize = 32;
pub const MAX_STEP_BYTES: usize = 65_536;
pub const MAX_COST_AMOUNT: u64 = 9_007_199_254_740_991;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepDescriptor {
    pub contract_version: u32,
    pub revision: u32,
    pub id: String,
    pub title: String,
    pub description: String,
    pub params: StepSchema,
    pub inputs: BTreeMap<String, StepPort>,
    pub outputs: BTreeMap<String, StepPort>,
    pub outcomes: Vec<String>,
    pub lifecycle: StepLifecycle,
    pub effects: StepEffects,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepPort {
    pub kind: PortKind,
    pub min_items: u16,
    pub max_items: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schema: Option<StepSchema>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PortKind {
    Text,
    Json,
    Image,
    Audio,
    Video,
    File,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepLifecycle {
    pub reconcile: bool,
    pub cancel: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StepEffects {
    pub side_effects: bool,
    pub potentially_paid: bool,
}

pub fn token(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 64
        && value.as_bytes()[0].is_ascii_lowercase()
        && value
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'_' | b'-'))
}

pub fn parse_descriptors(value: &Value) -> Result<Vec<StepDescriptor>, String> {
    let list = value
        .as_array()
        .ok_or("capabilities.graphNodes.steps: expected an array")?;
    if list.is_empty() || list.len() > MAX_STEP_DESCRIPTORS {
        return Err("capabilities.graphNodes.steps: declare 1..32 steps".into());
    }
    let mut steps = Vec::with_capacity(list.len());
    let mut ids = BTreeSet::new();
    for (index, value) in list.iter().enumerate() {
        let path = format!("capabilities.graphNodes.steps[{index}]");
        if value.to_string().len() > MAX_STEP_BYTES {
            return Err(format!("{path}: descriptor exceeds 64 KiB"));
        }
        let step: StepDescriptor =
            serde_json::from_value(value.clone()).map_err(|error| format!("{path}: {error}"))?;
        step.validate()
            .map_err(|error| format!("{path}: {error}"))?;
        if !ids.insert(step.id.clone()) {
            return Err(format!("{path}.id: duplicate plugin step id"));
        }
        steps.push(step);
    }
    Ok(steps)
}

impl StepDescriptor {
    pub fn validate(&self) -> Result<(), String> {
        StepSchema::from_value(&self.params.to_value())
            .map_err(|_| "invalid step parameter schema")?;
        let parts = self.id.split('.').collect::<Vec<_>>();
        if self.contract_version != 1
            || self.revision == 0
            || parts.len() < 2
            || !parts.iter().all(|part| token(part))
            || self.id.len() > 128
            || self.title.is_empty()
            || self.title.len() > 128
            || self.description.len() > 1024
            || self
                .title
                .chars()
                .chain(self.description.chars())
                .any(char::is_control)
        {
            return Err("plugin step identity/version/title is invalid".into());
        }
        if self.inputs.len() > 16
            || self.outputs.len() > 16
            || self.outcomes.is_empty()
            || self.outcomes.len() > 32
        {
            return Err("plugin step exceeds port/outcome bounds".into());
        }
        let mut outcomes = BTreeSet::new();
        for outcome in &self.outcomes {
            if !token(outcome)
                || !outcomes.insert(outcome)
                || matches!(
                    outcome.as_str(),
                    "waiting_for_owner_ok" | "unknown" | "canceled"
                )
            {
                return Err("invalid, duplicate or host-reserved plugin outcome".into());
            }
        }
        for (name, port) in self.inputs.iter().chain(&self.outputs) {
            if !token(name)
                || port.max_items == 0
                || port.max_items > 16
                || port.min_items > port.max_items
                || (port.kind == PortKind::Json) != port.schema.is_some()
            {
                return Err("invalid plugin step port".into());
            }
            if let Some(schema) = &port.schema {
                StepSchema::from_value(&schema.to_value())
                    .map_err(|_| "invalid step JSON schema")?;
            }
        }
        if serde_json::to_vec(self)
            .map_err(|_| "invalid descriptor")?
            .len()
            > MAX_STEP_BYTES
        {
            return Err("plugin step descriptor exceeds 64 KiB".into());
        }
        Ok(())
    }

    pub fn accepts_edge(&self, output: &StepPort, input: &str) -> bool {
        self.inputs.get(input).is_some_and(|port| {
            port.kind == output.kind
                && output.min_items >= port.min_items
                && output.max_items <= port.max_items
                && (port.schema.is_none() || port.schema == output.schema)
        })
    }
}
